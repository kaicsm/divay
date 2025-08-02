import pandas as pd
import os
import time
import torch
from transformers import pipeline, T5ForConditionalGeneration, T5Tokenizer
from tqdm.auto import tqdm
import gc
import warnings
warnings.filterwarnings("ignore")

def preprocess_text_for_translation(text):
    import re

    protected_tags = []
    def protect_tag(match):
        tag = match.group(0)
        placeholder = f"__TAG_{len(protected_tags)}__"
        protected_tags.append(tag)
        return placeholder

    protected_quotes = []
    def protect_quotes(match):
        quote = match.group(0)
        placeholder = f"__QUOTE_{len(protected_quotes)}__"
        protected_quotes.append(quote)
        return placeholder

    protected_placeholders = []
    def protect_placeholder(match):
        placeholder_text = match.group(0)
        placeholder = f"__PLACEHOLDER_{len(protected_placeholders)}__"
        protected_placeholders.append(placeholder_text)
        return placeholder

    text = re.sub(r'\[([^\]]+)\]', protect_tag, text)
    text = re.sub(r'"([^"]+)"', protect_quotes, text)
    text = re.sub(r'%\w+', protect_placeholder, text)

    return text, protected_tags, protected_quotes, protected_placeholders

def postprocess_translated_text(text, protected_tags, protected_quotes, protected_placeholders):
    for i, tag in enumerate(protected_tags):
        text = text.replace(f"__TAG_{i}__", tag)

    for i, quote in enumerate(protected_quotes):
        text = text.replace(f"__QUOTE_{i}__", quote)

    for i, placeholder in enumerate(protected_placeholders):
        text = text.replace(f"__PLACEHOLDER_{i}__", placeholder)

    return text

def translate_csv_batched(input_csv_path, output_csv_path, batch_size=32):
    print(f"Loading file: {input_csv_path}")
    input_df = pd.read_csv(input_csv_path)

    if 'translated_text' not in input_df.columns:
        input_df['translated_text'] = ''

    df = input_df.copy()
    df['translated_text'] = df['translated_text'].astype('object')

    if os.path.exists(output_csv_path):
        print(f"Existing output file found: {output_csv_path}. Attempting to resume translation...")
        try:
            existing_output_df = pd.read_csv(output_csv_path)

            existing_translations_map = {}
            for _, row in existing_output_df.iterrows():
                key = (row['unique_id'], row['original_text'])
                if pd.notna(row['translated_text']) and str(row['translated_text']).strip() != "":
                    existing_translations_map[key] = row['translated_text']

            for idx, row in df.iterrows():
                key = (row['unique_id'], row['original_text'])
                if key in existing_translations_map:
                    df.at[idx, 'translated_text'] = existing_translations_map[key]

            print("Existing translations loaded and merged.")

        except Exception as e:
            print(f"Error loading existing file: {e}. Starting a new translation...")
            df['translated_text'] = df['translated_text'].astype('object')
    else:
        print(f"Output file {output_csv_path} not found. Starting a new translation...")

    needs_translation_mask = df['translated_text'].isna() | (df['translated_text'].astype(str).str.strip() == "")
    indices_to_translate = df.index[needs_translation_mask].tolist()

    if not indices_to_translate:
        print("No new text to translate. The file is already fully translated.")
        df.to_csv(output_csv_path, index=False)
        return

    print("Initializing optimized translation model...")

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    print(f"Using device: {device}")

    model_name = "unicamp-dl/translation-en-pt-t5"

    tokenizer = T5Tokenizer.from_pretrained(model_name, legacy=False)

    if device.type == "cuda":
        model = T5ForConditionalGeneration.from_pretrained(
            model_name,
            torch_dtype=torch.float16,
            device_map="auto"
        )

        translator_pipeline = pipeline(
            "text2text-generation",
            model=model,
            tokenizer=tokenizer
        )
    else:
        model = T5ForConditionalGeneration.from_pretrained(
            model_name,
            torch_dtype=torch.float32
        )

        translator_pipeline = pipeline(
            "text2text-generation",
            model=model,
            tokenizer=tokenizer,
            device=device
        )

    print("Translation model initialized with optimizations.")

    total_lines_in_file = len(df)
    lines_already_translated = total_lines_in_file - len(indices_to_translate)

    print(f"Total lines in file: {total_lines_in_file}")
    print(f"Lines already translated: {lines_already_translated}")
    print(f"Lines to translate in this session: {len(indices_to_translate)}")
    print(f"Batch size: {batch_size}")

    save_frequency = max(1, len(indices_to_translate) // 20)

    start_time = time.time()

    with tqdm(total=total_lines_in_file, initial=lines_already_translated, unit="lines", desc="Translation Progress") as pbar:
        for i in range(0, len(indices_to_translate), batch_size):
            batch_start_time = time.time()

            batch_current_indices = indices_to_translate[i:i + batch_size]
            batch_original_texts = df.loc[batch_current_indices, 'original_text'].tolist()

            valid_texts = []
            valid_indices = []

            for idx, text in zip(batch_current_indices, batch_original_texts):
                if pd.notna(text) and str(text).strip() != "":
                    valid_texts.append(str(text).strip())
                    valid_indices.append(idx)
                else:
                    df.at[idx, 'translated_text'] = ""

            if valid_texts:
                preprocessed_data = []
                for text in valid_texts:
                    processed_text, tags, quotes, placeholders = preprocess_text_for_translation(text)
                    preprocessed_data.append((processed_text, tags, quotes, placeholders))

                prefixed_texts = ["translate English to Portuguese: " + data[0] for data in preprocessed_data]

                try:
                    batch_results = translator_pipeline(
                        prefixed_texts,
                        max_new_tokens=256,
                        num_beams=1,
                        do_sample=False,
                        batch_size=len(prefixed_texts)
                    )

                    if isinstance(batch_results, list):
                        translated_texts = [res['generated_text'] if isinstance(res, dict) else str(res) for res in batch_results]
                    else:
                        translated_texts = [str(batch_results)]

                    for idx, translated_text, (_, tags, quotes, placeholders) in zip(valid_indices, translated_texts, preprocessed_data):
                        final_text = postprocess_translated_text(translated_text.strip(), tags, quotes, placeholders)
                        df.at[idx, 'translated_text'] = final_text

                except Exception as e:
                    print(f"\nError translating batch {i//batch_size + 1}: {str(e)[:100]}...")
                    for idx, prefixed_text, (_, tags, quotes, placeholders) in zip(valid_indices, prefixed_texts, preprocessed_data):
                        try:
                            individual_result = translator_pipeline(
                                prefixed_text,
                                max_new_tokens=256,
                                num_beams=1,
                                do_sample=False
                            )

                            if isinstance(individual_result, list) and len(individual_result) > 0:
                                translated = individual_result[0]['generated_text'] if isinstance(individual_result[0], dict) else str(individual_result[0])
                            else:
                                translated = str(individual_result)

                            final_text = postprocess_translated_text(translated.strip(), tags, quotes, placeholders)
                            df.at[idx, 'translated_text'] = final_text
                        except:
                            df.at[idx, 'translated_text'] = "[TRANSLATION ERROR]"

            pbar.update(len(batch_current_indices))

            batch_time = time.time() - batch_start_time
            texts_per_second = len(batch_current_indices) / batch_time if batch_time > 0 else 0

            if i % (batch_size * 5) == 0:
                elapsed_time = time.time() - start_time
                remaining_batches = (len(indices_to_translate) - i - batch_size) // batch_size
                eta = (elapsed_time / ((i // batch_size) + 1)) * remaining_batches if i > 0 else 0

                pbar.set_postfix({
                    'Texts/s': f'{texts_per_second:.1f}',
                    'ETA': f'{eta/60:.1f}min'
                })

            if i % (save_frequency * batch_size) == 0:
                df.to_csv(output_csv_path, index=False)

                if device.type == "cuda":
                    torch.cuda.empty_cache()
                gc.collect()

    df.to_csv(output_csv_path, index=False)

    total_time = time.time() - start_time
    total_translated = len(indices_to_translate)
    avg_speed = total_translated / total_time if total_time > 0 else 0

    print(f"\nTranslation complete!")
    print(f"Total time: {total_time/60:.1f} minutes")
    print(f"Average speed: {avg_speed:.1f} texts/second")
    print(f"File saved to: {output_csv_path}")

def optimize_batch_size():
    if not torch.cuda.is_available():
        return 16

    gpu_memory = torch.cuda.get_device_properties(0).total_memory / 1024**3

    if gpu_memory >= 24:
        return 48
    elif gpu_memory >= 16:
        return 32
    elif gpu_memory >= 12:
        return 24
    elif gpu_memory >= 8:
        return 16
    else:
        return 8

if __name__ == "__main__":
    input_file = "text.csv"
    output_file = "text_translated_colab.csv"

    optimal_batch_size = optimize_batch_size()
    print(f"Optimized batch size: {optimal_batch_size}")

    translate_csv_batched(input_file, output_file, batch_size=optimal_batch_size)

# Divay - Morrowind Translation Tool

Divay is a command-line tool written in Rust for extracting and translating text from The Elder Scrolls III: Morrowind game files (.esm and .esp). It provides a complete workflow for creating localized versions of Morrowind mods and the base game by extracting translatable text to CSV files, facilitating translation, and reinserting the translated content back into the game files.

## Features

- **Text Extraction**: Extract translatable text from ESM files to CSV format
- **Smart Filtering**: Automatically filters out scripts, code patterns, and non-translatable content
- **Text Injection**: Reinsert translated text back into game files while preserving binary structure
- **Selective Processing**: Extract only specific record types (BOOK, INFO, GMST, etc.)
- **Python Integration**: Optional automated translation using transformer models via Google Colab

## Installation

### Prerequisites

- Rust (latest stable version)
- Cargo package manager

### Building from Source

```bash
git clone https://github.com/kaicsm/divay
cd divay/cli
cargo build --release
```

The compiled binary will be available at `target/release/divay`.

## Usage

### Basic Commands

Divay provides two main commands: `extract` and `inject`.

#### Extracting Text

Extract all translatable text from a Morrowind file:

```bash
divay extract -i Morrowind.esm -o text.csv
```

Extract only specific record types:

```bash
divay extract -i Morrowind.esm -o dialogue.csv --types INFO
```

#### Injecting Translations

Inject translated text back into the game file:

```bash
divay inject -i Morrowind.esm -c translated_text.csv -o Morrowind_translated.esm
```

## Python Translation Pipeline

Divay includes a Python script (`tools/translate_colab.py`) for automated translation using transformer models. This script is optimized for Google Colab environments.

### Setting up Translation

1. Upload your extracted CSV file to Google Colab.

2. Install required packages:

```python
!pip install torch transformers pandas tqdm
```

3. Run the translation script in your Colab notebook.

## Workflow Example

Here's a complete translation workflow:

1. Extract text from the original file:
   ```bash
   divay extract -i "Morrowind.esm" -o "original_text.csv"
   ```

2. Translate the CSV file manually or using the Python script.

3. Inject translations back:
   ```bash
   divay inject -i "Morrowind.esm" -c "translated_text.csv" -o "Morrowind_Portuguese.esm"
   ```

4. Test the translated file by replacing the original in your Morrowind installation.

## License

This project is licensed under the MIT License. See the LICENSE file for details.

## Acknowledgments

- The Elder Scrolls III: Morrowind by Bethesda Game Studios
- OpenMW project for documentation

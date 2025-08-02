use clap::{Parser, Subcommand};

mod extractor;
mod injector;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extracts translatable text from an ESM/ESP file into a CSV.
    Extract {
        /// Path to the input .esm or .esp file.
        #[arg(short, long)]
        input: String,

        /// Path to the output .csv file.
        #[arg(short, long)]
        output: String,

        /// Comma-separated list of record types to extract (e.g., BOOK,INFO,GMST).
        #[arg(short, long, value_delimiter = ',', value_parser = clap::builder::NonEmptyStringValueParser::new())]
        types: Option<Vec<String>>,
    },
    /// Injects translated text back into an ESM/ESP file.
    Inject {
        /// Path to the original .esm or .esp file.
        #[arg(short, long)]
        input: String,

        /// Path to the .csv file with translations.
        #[arg(short, long)]
        csv: String,

        /// Path to the output .esm or .esp file.
        #[arg(short, long)]
        output: String,

        /// Create a patch ESP instead of a full replacement.
        #[arg(long)]
        patch: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Extract {
            input,
            output,
            types,
        } => {
            let filter_types = types.clone().map(|t| t.into_iter().collect());
            extractor::extract(input.as_ref(), output.as_ref(), filter_types.as_ref())?;
        }
        Commands::Inject {
            input,
            csv,
            output,
            patch,
        } => {
            injector::inject(input.as_ref(), csv.as_ref(), output.as_ref(), *patch)?;
        }
    }
    Ok(())
}

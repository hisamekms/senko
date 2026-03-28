use clap::Parser;
use localflow::presentation::cli::{Cli, OutputFormat};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let output_format = cli.output.clone();

    if let Err(e) = localflow::presentation::cli::run(cli).await {
        match output_format {
            OutputFormat::Json => {
                println!("{}", serde_json::json!({"error": format!("{:#}", e)}));
                std::process::exit(1);
            }
            OutputFormat::Text => {
                eprintln!("Error: {:#}", e);
                std::process::exit(1);
            }
        }
    }
}

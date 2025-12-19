//! CLI for per-link Shapley value estimation.
//!
//! Computes Shapley values for each individual link owned by a specific operator.
//!
//! # Usage
//!
//! ```bash
//! link-estimate \
//!     --private-links private_links.csv \
//!     --devices devices.csv \
//!     --public-links public_links.csv \
//!     --demands demand.csv \
//!     --operator "OperatorName"
//! ```

use std::{fs::File, io, path::PathBuf, process::ExitCode};

use clap::Parser;
use tabled::{Table, settings::Style};

use network_shapley::{
    error::{Result, ShapleyError},
    link_estimate::LinkEstimateInput,
    types::{Demands, Devices, PrivateLinks, PublicLinks},
};

#[derive(Parser, Debug)]
#[command(name = "link-estimate")]
#[command(about = "Compute per-link Shapley values for a specific operator")]
#[command(version)]
struct Args {
    /// Path to private links CSV file
    #[arg(short = 'p', long)]
    private_links: PathBuf,

    /// Path to devices CSV file
    #[arg(short = 'd', long)]
    devices: PathBuf,

    /// Path to public links CSV file
    #[arg(short = 'u', long)]
    public_links: PathBuf,

    /// Path to demands CSV file
    #[arg(short = 'm', long)]
    demands: PathBuf,

    /// Operator to compute per-link values for
    #[arg(short = 'o', long)]
    operator: String,

    /// Contiguity bonus (latency penalty for mixing public/private)
    #[arg(short = 'c', long, default_value = "5.0")]
    contiguity_bonus: f64,

    /// Demand multiplier to scale traffic
    #[arg(short = 'x', long, default_value = "1.0")]
    demand_multiplier: f64,

    /// Output format: table (default), csv, or json
    #[arg(short = 'f', long, default_value = "table")]
    format: String,
}

fn read_csv<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<Vec<T>> {
    let file = File::open(path).map_err(|e| {
        ShapleyError::Validation(format!("Failed to open {}: {}", path.display(), e))
    })?;

    let mut rdr = csv::Reader::from_reader(file);
    let mut items = Vec::new();

    for (idx, result) in rdr.deserialize().enumerate() {
        let item: T = result.map_err(|e| {
            ShapleyError::Validation(format!(
                "Failed to parse row {} in {}: {}",
                idx + 1,
                path.display(),
                e
            ))
        })?;
        items.push(item);
    }

    Ok(items)
}

fn run(args: Args) -> Result<()> {
    // Read input files
    let private_links: PrivateLinks = read_csv(&args.private_links)?;
    let devices: Devices = read_csv(&args.devices)?;
    let public_links: PublicLinks = read_csv(&args.public_links)?;
    let demands: Demands = read_csv(&args.demands)?;

    eprintln!(
        "[INFO] Loaded {} private links, {} devices, {} public links, {} demands",
        private_links.len(),
        devices.len(),
        public_links.len(),
        demands.len()
    );
    eprintln!(
        "[INFO] Computing per-link values for operator: {}",
        args.operator
    );

    // Compute
    let input = LinkEstimateInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_focus: args.operator,
        contiguity_bonus: args.contiguity_bonus,
        demand_multiplier: args.demand_multiplier,
    };

    let result = input.compute()?;

    // Output
    match args.format.as_str() {
        "csv" => {
            let mut wtr = csv::Writer::from_writer(io::stdout());
            for lv in &result {
                wtr.serialize(lv)
                    .map_err(|e| ShapleyError::Validation(format!("Failed to write CSV: {}", e)))?;
            }
            wtr.flush()
                .map_err(|e| ShapleyError::Validation(format!("Failed to flush CSV: {}", e)))?;
        }
        "json" => {
            let json = serde_json::to_string_pretty(&result).map_err(|e| {
                ShapleyError::Validation(format!("Failed to serialize to JSON: {}", e))
            })?;
            println!("{}", json);
        }
        _ => {
            // table format (default)
            if result.is_empty() {
                println!("No links found for the specified operator.");
            } else {
                let table = Table::new(&result).with(Style::rounded()).to_string();
                println!("{}", table);
            }
        }
    }

    eprintln!("[INFO] Found {} links with values", result.len());

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();

    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[ERROR] {}", e);
            ExitCode::FAILURE
        }
    }
}

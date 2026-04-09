use std::{
    io::{self, Read},
    process::ExitCode,
};

use network_shapley::shapley::{ShapleyInput, ShapleyValue};
use serde::Serialize;

#[derive(Serialize)]
struct OperatorValue {
    operator: String,
    value: f64,
    proportion: f64,
}

impl From<(String, ShapleyValue)> for OperatorValue {
    fn from((operator, sv): (String, ShapleyValue)) -> Self {
        Self {
            operator,
            value: sv.value,
            proportion: sv.proportion,
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input_json = String::new();
    io::stdin().read_to_string(&mut input_json)?;

    let input: ShapleyInput = serde_json::from_str(&input_json)?;

    let result = input.compute()?;

    let output: Vec<OperatorValue> = result.into_iter().map(OperatorValue::from).collect();

    let json = serde_json::to_string(&output)?;
    println!("{json}");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

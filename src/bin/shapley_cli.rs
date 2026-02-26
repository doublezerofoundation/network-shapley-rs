use network_shapley::shapley::{ShapleyInput, ShapleyValue};
use serde::Serialize;
use std::io::{self, Read};

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

fn main() {
    let mut input_json = String::new();
    io::stdin()
        .read_to_string(&mut input_json)
        .expect("failed to read stdin");

    let input: ShapleyInput =
        serde_json::from_str(&input_json).expect("failed to parse input JSON");

    let result = input.compute().expect("shapley computation failed");

    let output: Vec<OperatorValue> = result.into_iter().map(OperatorValue::from).collect();

    let json = serde_json::to_string(&output).expect("failed to serialize output");
    println!("{json}");
}

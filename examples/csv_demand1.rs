use network_shapley::{
    error::Result,
    shapley::ShapleyInput,
    types::{Demand, Demands, Device, Devices, PrivateLink, PrivateLinks, PublicLink, PublicLinks},
};
use std::fs::File;
use tabled::{builder::Builder as TableBuilder, settings::Style};

fn read_pvt_links(file_path: &str) -> Result<PrivateLinks> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut links = vec![];
    for result in rdr.deserialize() {
        let pvt_link: PrivateLink = result.unwrap();
        links.push(pvt_link);
    }
    Ok(links)
}

fn read_devices(file_path: &str) -> Result<Devices> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut devices = vec![];
    for result in rdr.deserialize() {
        let device: Device = result.unwrap();
        devices.push(device);
    }
    Ok(devices)
}

fn read_pub_links(file_path: &str) -> Result<PublicLinks> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut links = vec![];
    for result in rdr.deserialize() {
        let link: PublicLink = result.unwrap();
        links.push(link);
    }
    Ok(links)
}

fn read_demands(file_path: &str) -> Result<Demands> {
    let file = File::open(file_path).unwrap();
    let mut rdr = csv::Reader::from_reader(file);
    let mut demands = vec![];
    for result in rdr.deserialize() {
        let demand: Demand = result.unwrap();
        demands.push(demand);
    }
    Ok(demands)
}

fn main() -> Result<()> {
    let private_links = read_pvt_links("tests/private_links.csv").unwrap();
    let devices = read_devices("tests/devices.csv").unwrap();
    let public_links = read_pub_links("tests/public_links.csv").unwrap();
    let demand = read_demands("tests/demand1.csv").unwrap();

    let input = ShapleyInput {
        private_links: private_links.clone(),
        devices: devices.clone(),
        demands: demand,
        public_links: public_links.clone(),
        operator_uptime: 0.98,
        contiguity_bonus: 5.0,
        demand_multiplier: 1.2,
    };

    let result = input.compute()?;

    let table = TableBuilder::from(result)
        .build()
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{table}");

    Ok(())
}

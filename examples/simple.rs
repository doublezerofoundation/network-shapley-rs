use network_shapley::{
    error::Result,
    shapley::ShapleyInput,
    types::{Demand, Demands, Device, Devices, PrivateLink, PrivateLinks, PublicLink, PublicLinks},
};
use tabled::{Table, settings::Style};

fn build_pvt_links() -> PrivateLinks {
    let pl1 = PrivateLink::new(
        "SIN1".to_string(),
        "FRA1".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    );
    let pl2 = PrivateLink::new("FRA1".to_string(), "AMS1".to_string(), 3.0, 10.0, 1.0, None);
    let pl3 = PrivateLink::new("FRA1".to_string(), "LON1".to_string(), 5.0, 10.0, 1.0, None);
    vec![pl1, pl2, pl3]
}

fn build_devices() -> Devices {
    let d1 = Device::new("SIN1".to_string(), 1, "Alpha".to_string());
    let d2 = Device::new("FRA1".to_string(), 1, "Alpha".to_string());
    let d3 = Device::new("AMS1".to_string(), 1, "Beta".to_string());
    let d4 = Device::new("LON1".to_string(), 1, "Beta".to_string());
    vec![d1, d2, d3, d4]
}

fn build_pub_links() -> PublicLinks {
    let pl1 = PublicLink::new("SIN".to_string(), "FRA".to_string(), 100.0);
    let pl2 = PublicLink::new("SIN".to_string(), "AMS".to_string(), 102.0);
    let pl3 = PublicLink::new("FRA".to_string(), "LON".to_string(), 7.0);
    let pl4 = PublicLink::new("FRA".to_string(), "AMS".to_string(), 5.0);
    vec![pl1, pl2, pl3, pl4]
}

fn build_demands() -> Demands {
    let d1 = Demand::new("SIN".to_string(), "AMS".to_string(), 1, 1.0, 1.0, 1, true);
    let d2 = Demand::new("SIN".to_string(), "LON".to_string(), 5, 1.0, 2.0, 1, true);
    let d3 = Demand::new("AMS".to_string(), "LON".to_string(), 2, 3.0, 1.0, 2, false);
    let d4 = Demand::new("AMS".to_string(), "FRA".to_string(), 1, 3.0, 1.0, 2, false);
    vec![d1, d2, d3, d4]
}

fn main() -> Result<()> {
    let private_links = build_pvt_links();
    let devices = build_devices();
    let public_links = build_pub_links();
    let demands = build_demands();

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 0.98,
        contiguity_bonus: 5.0,
        demand_multiplier: 1.0,
    };

    let result = input.compute()?;
    let table = Table::new(result.values)
        .with(Style::psql().remove_horizontals())
        .to_string();
    println!("{table}");

    Ok(())
}

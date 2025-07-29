use std::{collections::HashMap, env, io, sync::Arc};

use a_sabr::{
    bundle::Bundle,
    contact_manager::legacy::evl::EVLManager,
    contact_plan::from_ion_file::IONContactPlan,
    node_manager::none::NoManagement,
    routing::{aliases::build_generic_router, Router},
    types::{Date, NodeID},
};
use chrono::{DateTime, Utc};

pub struct PredictionConfig {
    ion_to_node_id: HashMap<String, NodeID>,
    router: Box<dyn Router<NoManagement, EVLManager> + 'static + Sync + Send>,
    cp_start_time: f64,
}

fn extract_ion_id_from_bp_address(bp_address: &str) -> String {
    if let Some(after_ipn) = bp_address.strip_prefix("ipn:") {
        if let Some(dot_pos) = after_ipn.find('.') {
            return after_ipn[..dot_pos].to_string();
        }
    }
    bp_address.to_string()
}

pub fn f64_to_utc(timestamp: f64) -> DateTime<Utc> {
    let secs = timestamp.trunc() as i64;
    let nsecs = ((timestamp.fract()) * 1_000_000_000.0).round() as u32;
    let naive = DateTime::from_timestamp(secs, nsecs).expect("Invalid timestamp");
    DateTime::from_naive_utc_and_offset(naive.naive_utc(), Utc)
}

impl PredictionConfig {
    pub fn try_init() -> io::Result<Self> {
        let cp_path: String = env::var("CP_PATH")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "CP_PATH not set"))?;

        let (nodes, contacts) = IONContactPlan::parse::<NoManagement, EVLManager>(&cp_path)?;

        let node_index_map: HashMap<String, NodeID> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.get_node_name().to_string(), index as NodeID))
            .collect();

        let router = build_generic_router::<NoManagement, EVLManager>(
            "CgrFirstEndingContactParenting",
            nodes,
            contacts,
            None,
        );

        let router: Box<dyn Router<NoManagement, EVLManager> + Send + Sync> =
            unsafe { std::mem::transmute(router) };

        let cp_start_time = Utc::now().timestamp() as f64;

        Ok(PredictionConfig {
            ion_to_node_id: node_index_map,
            router,
            cp_start_time,
        })
    }

    pub fn get_node_id(&self, ion_id: &str) -> Option<NodeID> {
        self.ion_to_node_id.get(ion_id).copied()
    }

    pub fn predict(
        &mut self,
        source_eid: &str,
        dest_eid: &str,
        message_size: f64,
    ) -> io::Result<DateTime<Utc>> {
        let source_ion = extract_ion_id_from_bp_address(source_eid);
        let dest_ion = extract_ion_id_from_bp_address(dest_eid);

        let source_node_id = self.get_node_id(&source_ion).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("[PBAT-CONFIG]: Source ION ID '{source_ion}' not found in contact plan"),
            )
        })?;

        let dest_node_id = self.get_node_id(&dest_ion).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("[PBAT-CONFIG]: Destination ION ID '{dest_ion}' not found in contact plan"),
            )
        })?;

        let bundle = Bundle {
            source: source_node_id,
            destinations: vec![dest_node_id],
            priority: 0,
            size: message_size,
            expiration: Date::MAX,
        };

        let excluded_nodes = vec![];

        let cp_send_time = Utc::now().timestamp() as f64 - self.cp_start_time;

        match self
            .router
            .route(bundle.source, &bundle, cp_send_time, &excluded_nodes)
        {
            Some(routing_output) => {
                // println!("Route found from ION {} to ION {}", source_ion, dest_ion);
                // Only display the last element
                if let Some((_contact_ptr, (_contact, route_stages))) =
                    routing_output.first_hops.iter().last()
                {
                    if let Some(last_stage) = route_stages.last() {
                        // Create a borrow and use it consistently
                        let last_stage_borrowed = last_stage.borrow();

                        let delay = last_stage_borrowed.at_time;

                        // println!(
                        //     "CP start time in UTC: {:?}",
                        //     DateTime::<Utc>::from_timestamp_millis(self.cp_start_time as i64)
                        // );
                        // println!(
                        //     "Delivery time in UTC: {:?}",
                        //     DateTime::<Utc>::from_timestamp_millis((delay + self.cp_start_time) as i64)
                        // );
                        // println!("CP send time: {}", cp_send_time);
                        // println!("Delay in seconds: {}", delay);
                        return Ok(f64_to_utc(delay + self.cp_start_time));
                    }
                }
                Err(io::Error::other(
                    "Route found but no route stages available",
                ))
            }
            None => {
                // eprintln!("No route found from ION {} to ION {}", source_ion, dest_ion);
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("No route found from ION {source_ion} to ION {dest_ion}"),
                ))
            }
        }
    }
}

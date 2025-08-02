use crate::{
    db::{simple_vec::SimpleVecDB, ChatDataBase},
    dtchat::{ASabrInitState, Peer},
    prediction::PredictionConfig,
};
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer,
};
use socket_engine::endpoint::Endpoint;
use std::fmt;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub enum DbType {
    YamlVec,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub db_type: DbType,
    pub cp_path: Option<String>,
}
#[derive(Clone, Debug)]
pub struct EndpointWrapper(pub Endpoint);

impl<'de> Deserialize<'de> for EndpointWrapper {
    fn deserialize<D>(deserializer: D) -> Result<EndpointWrapper, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EndpointVisitor;

        impl<'de> Visitor<'de> for EndpointVisitor {
            type Value = EndpointWrapper;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string like 'tcp 127.0.0.1:8000'")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Endpoint::from_str(v)
                    .map(EndpointWrapper)
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_str(EndpointVisitor)
    }
}

// Optional: Convert from wrapper to inner type
impl From<EndpointWrapper> for Endpoint {
    fn from(wrapper: EndpointWrapper) -> Self {
        wrapper.0
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawPeer {
    pub uuid: String,
    pub name: String,
    pub endpoints: Vec<EndpointWrapper>,
    pub color: String,
}

#[derive(Debug, Deserialize)]
pub struct YamlVec {
    pub peer_list: Vec<RawPeer>,
}

impl From<RawPeer> for Peer {
    fn from(raw: RawPeer) -> Self {
        Peer {
            uuid: raw.uuid,
            name: raw.name,
            color: raw.color,
            endpoints: raw.endpoints.into_iter().map(|e| e.into()).collect(),
        }
    }
}

pub struct AppConfig {}

impl AppConfig {
    const DEFAULT_CONFIG_PATH_VALUE: &str = "default.yaml";
    const DEFAULT_CONFIG_PATH_ENV_VAR: &str = "CONFIG_PATH";

    pub fn new() -> (Box<dyn ChatDataBase>, ASabrInitState) {
        let config_file = match std::env::var(Self::DEFAULT_CONFIG_PATH_ENV_VAR) {
            Ok(path) => path,
            Err(_) => {
                println!(
                    "{} is not set, trying with {}",
                    Self::DEFAULT_CONFIG_PATH_ENV_VAR,
                    Self::DEFAULT_CONFIG_PATH_VALUE
                );
                Self::DEFAULT_CONFIG_PATH_VALUE.to_string()
            }
        };

        let conf: Config = Self::from_file(&config_file).unwrap_or_else(|e| {
            panic!("Failed to load configuration from '{config_file}': {e}");
        });

        let db = match conf.db_type {
            DbType::YamlVec => Self::yaml_vec(&config_file),
        };
        let cp_path_unwrapped = match conf.cp_path {
            Some(cp) => cp,
            None => {
                return (db, ASabrInitState::Disabled);
            }
        };

        let pred_res = PredictionConfig::try_init(cp_path_unwrapped);
        let pred_opt = match pred_res {
            Ok(pred_conf) => ASabrInitState::Enabled(pred_conf),
            Err(err) => ASabrInitState::Error(err.to_string()),
        };
        (db, pred_opt)
    }

    fn yaml_vec(config_file: &str) -> Box<dyn ChatDataBase> {
        const PEER_ENV_VAR: &str = "PEER_UUID";

        let local_peer_uuid = match std::env::var(PEER_ENV_VAR) {
            Ok(uuid) => uuid,
            Err(_) => {
                panic!("{} must be set with the YamlVec Method", PEER_ENV_VAR);
            }
        };

        let conf: YamlVec = Self::from_file(&config_file).unwrap_or_else(|e| {
            panic!("Failed to load configuration from '{config_file}': {e}");
        });

        let mut peers: Vec<Peer> = Vec::new();
        let mut local_peer_opt = None;
        for p in conf.peer_list {
            if p.uuid == local_peer_uuid {
                local_peer_opt = Some(p.clone())
            } else {
                peers.push(Peer::from(p));
            }
        }
        let Some(local_peer) = local_peer_opt else {
            panic!("Failed identify localpeer with uuid '{local_peer_uuid}'")
        };

        Box::new(SimpleVecDB::new(Vec::new(), Peer::from(local_peer), peers))
    }

    pub fn from_file<T, P>(path: P) -> Result<T, Box<dyn std::error::Error>>
    where
        T: for<'de> Deserialize<'de>,
        P: AsRef<std::path::Path>,
    {
        let content = fs::read_to_string(path)?;
        let config: T = serde_yaml::from_str(&content)?;
        Ok(config)
    }
}

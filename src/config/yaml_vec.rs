use crate::{
    config::AppConfig,
    db::{simple_vec::SimpleVecDB, ChatDataBase},
    dtchat::Peer,
};
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer,
};
use socket_engine::endpoint::Endpoint;
use std::fmt;

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

#[derive(Debug, Deserialize)]
pub struct YamlVec {
    pub peer_list: Vec<RawPeer>,
}

impl YamlVec {
    pub fn new(config_file: &str) -> Box<dyn ChatDataBase> {
        const PEER_ENV_VAR: &str = "PEER_UUID";

        let local_peer_uuid = match std::env::var(PEER_ENV_VAR) {
            Ok(uuid) => uuid,
            Err(_) => {
                panic!("{} must be set with the YamlVec Method", PEER_ENV_VAR);
            }
        };

        let conf: YamlVec = AppConfig::from_file(&config_file).unwrap_or_else(|e| {
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
}

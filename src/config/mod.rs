use crate::{
    config::yaml_vec::YamlVec, db::ChatDataBase, dtchat::ASabrInitState,
    prediction::PredictionConfig,
};
use serde::Deserialize;
use std::fs;

mod yaml_vec;

#[derive(Debug, Clone, Deserialize)]
pub enum DbType {
    YamlVec,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub db_type: DbType,
    pub file_reception_dir: Option<String>,
    pub cp_path: Option<String>,
}

pub struct AppConfig {}

impl AppConfig {
    const DEFAULT_FILE_RECEPTION_DIR: &str = "./";
    const DEFAULT_CONFIG_PATH_VALUE: &str = "default.yaml";
    const DEFAULT_CONFIG_PATH_ENV_VAR: &str = "CONFIG_PATH";

    pub fn new() -> (Box<dyn ChatDataBase>, ASabrInitState, String) {
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
            DbType::YamlVec => YamlVec::new(&config_file),
        };

        let file_reception_path = match conf.file_reception_dir {
            Some(path) => path,
            None => Self::DEFAULT_FILE_RECEPTION_DIR.to_string(),
        };

        let cp_path_unwrapped = match conf.cp_path {
            Some(cp) => cp,
            None => {
                return (db, ASabrInitState::Disabled, file_reception_path);
            }
        };

        let pred_res = PredictionConfig::try_init(cp_path_unwrapped);
        let pred_opt = match pred_res {
            Ok(pred_conf) => ASabrInitState::Enabled(pred_conf),
            Err(err) => ASabrInitState::Error(err.to_string()),
        };
        (db, pred_opt, file_reception_path)
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

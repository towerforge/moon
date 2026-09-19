use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use futures_util::future::join_all;

use crate::config::Config;
use crate::error::{ConfigError, ProviderError};
use crate::model_ref;
use crate::provider::{Provider, ProviderFactory};
use crate::types::ModelInfo;

/// Factories by kind and instances by id. The only place that knows which
/// providers exist.
#[derive(Default)]
pub struct Registry {
    factories: HashMap<&'static str, Box<dyn ProviderFactory>>,
    providers: BTreeMap<String, Arc<dyn Provider>>,
    /// Configured providers that could not be built, with the reason.
    disabled: BTreeMap<String, String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factory: Box<dyn ProviderFactory>) {
        self.factories.insert(factory.kind(), factory);
    }

    pub fn kinds(&self) -> Vec<&'static str> {
        let mut k: Vec<_> = self.factories.keys().copied().collect();
        k.sort_unstable();
        k
    }

    /// Builds every instance from `[providers.*]`. An unknown `type` is an
    /// error; a provider that cannot be built (for example, missing its
    /// environment variable) is left disabled.
    pub fn build_all(&mut self, cfg: &Config) -> Result<(), ConfigError> {
        for (id, pcfg) in &cfg.providers {
            if !pcfg.enabled {
                self.disabled
                    .insert(id.clone(), "disabled in the configuration".into());
                continue;
            }
            let Some(factory) = self.factories.get(pcfg.kind.as_str()) else {
                return Err(ConfigError::UnknownKind {
                    id: id.clone(),
                    kind: pcfg.kind.clone(),
                    known: self.kinds().join(", "),
                });
            };
            match factory.build(id, pcfg) {
                Ok(p) => {
                    self.providers.insert(id.clone(), p);
                }
                Err(ConfigError::Provider { detail, .. }) => {
                    self.disabled.insert(id.clone(), detail);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).cloned()
    }

    pub fn has(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn providers(&self) -> impl Iterator<Item = (&String, &Arc<dyn Provider>)> {
        self.providers.iter()
    }

    pub fn disabled(&self) -> &BTreeMap<String, String> {
        &self.disabled
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Asks every provider for its models, in parallel.
    pub async fn list_all_models(&self) -> Vec<(String, Result<Vec<ModelInfo>, ProviderError>)> {
        let futs = self.providers.iter().map(|(id, p)| {
            let id = id.clone();
            let p = p.clone();
            async move { (id, p.list_models().await) }
        });
        join_all(futs).await
    }

    /// Resolves `provider/model` or `model` (with the default provider) to an
    /// instance and a model id. Does not check that the model exists.
    pub fn resolve(
        &self,
        spec: &str,
        default_provider: Option<&str>,
    ) -> Result<(Arc<dyn Provider>, String), String> {
        let (prov, model) = model_ref::split(spec, |p| self.has(p));
        let prov = match prov {
            Some(p) => p.to_string(),
            None => match default_provider {
                Some(p) if self.has(p) => p.to_string(),
                _ => {
                    if self.providers.len() == 1 {
                        self.providers.keys().next().cloned().unwrap()
                    } else if self.providers.is_empty() {
                        return Err("no provider available".into());
                    } else {
                        return Err(format!(
                            "`{spec}` names no provider and there are several ({}): use provider/model",
                            self.ids().join(", ")
                        ));
                    }
                }
            },
        };
        let p = self
            .get(&prov)
            .ok_or_else(|| format!("unknown provider: {prov}"))?;
        Ok((p, model.to_string()))
    }
}

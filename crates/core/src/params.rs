//! `/params key=value …`: apply changes to `GenerationParams` from text.

use crate::types::GenerationParams;

pub const KEYS: &[&str] = &[
    "temperature",
    "top_p",
    "max_tokens",
    "num_ctx",
    "stop",
    "think",
];

impl GenerationParams {
    /// Returns a copy with whichever values of `other` are set.
    pub fn merged_with(&self, other: &GenerationParams) -> GenerationParams {
        let mut out = self.clone();
        if other.temperature.is_some() {
            out.temperature = other.temperature;
        }
        if other.top_p.is_some() {
            out.top_p = other.top_p;
        }
        if other.max_tokens.is_some() {
            out.max_tokens = other.max_tokens;
        }
        if other.num_ctx.is_some() {
            out.num_ctx = other.num_ctx;
        }
        if !other.stop.is_empty() {
            out.stop = other.stop.clone();
        }
        if other.think.is_some() {
            out.think = other.think;
        }
        for (k, v) in &other.extra {
            out.extra.insert(k.clone(), v.clone());
        }
        out
    }

    /// Applies `key=value`. An empty value or `none` clears the key.
    pub fn apply(&mut self, key: &str, value: &str) -> Result<(), String> {
        let value = value.trim();
        let none = value.is_empty() || value.eq_ignore_ascii_case("none");
        fn num<T: std::str::FromStr>(key: &str, v: &str) -> Result<T, String> {
            v.parse::<T>()
                .map_err(|_| format!("`{key}` needs a number, not `{v}`"))
        }
        match key {
            "temperature" => self.temperature = if none { None } else { Some(num(key, value)?) },
            "top_p" => self.top_p = if none { None } else { Some(num(key, value)?) },
            "max_tokens" => self.max_tokens = if none { None } else { Some(num(key, value)?) },
            "num_ctx" => self.num_ctx = if none { None } else { Some(num(key, value)?) },
            "think" => {
                self.think = if none {
                    None
                } else {
                    Some(match value {
                        "true" | "on" | "1" | "yes" | "si" | "sí" => true,
                        "false" | "off" | "0" | "no" => false,
                        other => return Err(format!("`think` needs true/false, not `{other}`")),
                    })
                }
            }
            "stop" => {
                self.stop = if none {
                    Vec::new()
                } else {
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                }
            }
            other => {
                if none {
                    self.extra.remove(other);
                } else {
                    let v = value
                        .parse::<i64>()
                        .map(toml::Value::Integer)
                        .or_else(|_| value.parse::<f64>().map(toml::Value::Float))
                        .or_else(|_| value.parse::<bool>().map(toml::Value::Boolean))
                        .unwrap_or_else(|_| toml::Value::String(value.to_string()));
                    self.extra.insert(other.to_string(), v);
                }
            }
        }
        Ok(())
    }

    /// Short representation for display: `temperature=0.7 num_ctx=16384`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = self.temperature {
            parts.push(format!("temperature={v}"));
        }
        if let Some(v) = self.top_p {
            parts.push(format!("top_p={v}"));
        }
        if let Some(v) = self.max_tokens {
            parts.push(format!("max_tokens={v}"));
        }
        if let Some(v) = self.num_ctx {
            parts.push(format!("num_ctx={v}"));
        }
        if !self.stop.is_empty() {
            parts.push(format!("stop={}", self.stop.join(",")));
        }
        if let Some(v) = self.think {
            parts.push(format!("think={v}"));
        }
        for (k, v) in &self.extra {
            parts.push(format!("{k}={v}"));
        }
        if parts.is_empty() {
            "(provider defaults)".to_string()
        } else {
            parts.join(" ")
        }
    }
}

/// Splits `a=1 b=2` into pairs. Returns an error on a token without `=`.
pub fn parse_pairs(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        let Some((k, v)) = tok.split_once('=') else {
            return Err(format!("`{tok}` is not key=value"));
        };
        out.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aplica_y_borra() {
        let mut p = GenerationParams::default();
        p.apply("temperature", "0.2").unwrap();
        p.apply("num_ctx", "16384").unwrap();
        p.apply("think", "on").unwrap();
        p.apply("stop", "a, b").unwrap();
        p.apply("mirostat", "2").unwrap();
        assert_eq!(p.temperature, Some(0.2));
        assert_eq!(p.num_ctx, Some(16384));
        assert_eq!(p.think, Some(true));
        assert_eq!(p.stop, vec!["a", "b"]);
        assert_eq!(p.extra.get("mirostat"), Some(&toml::Value::Integer(2)));
        p.apply("temperature", "none").unwrap();
        p.apply("mirostat", "").unwrap();
        assert_eq!(p.temperature, None);
        assert!(p.extra.is_empty());
        assert!(p.apply("num_ctx", "muchos").is_err());
    }

    #[test]
    fn pares() {
        assert_eq!(
            parse_pairs("a=1 b=x").unwrap(),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "x".to_string())
            ]
        );
        assert!(parse_pairs("a").is_err());
    }

    #[test]
    fn merge_respeta_lo_fijado() {
        let base = GenerationParams {
            temperature: Some(0.7),
            num_ctx: Some(8192),
            ..Default::default()
        };
        let over = GenerationParams {
            temperature: Some(0.1),
            ..Default::default()
        };
        let m = base.merged_with(&over);
        assert_eq!(m.temperature, Some(0.1));
        assert_eq!(m.num_ctx, Some(8192));
    }
}

//! Qualified ids `provider/model`. Since a model id may contain slashes
//! ("meta-llama/llama-3"), it is only split at the first slash if what
//! precedes it is a known provider.

/// Splits `spec` into (provider, model). `known` tells whether a provider id exists.
pub fn split(spec: &str, known: impl Fn(&str) -> bool) -> (Option<&str>, &str) {
    match spec.split_once('/') {
        Some((p, m)) if !p.is_empty() && !m.is_empty() && known(p) => (Some(p), m),
        _ => (None, spec),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corta_solo_con_proveedor_conocido() {
        let known = |p: &str| p == "ollama";
        assert_eq!(split("ollama/qwen", known), (Some("ollama"), "qwen"));
        assert_eq!(
            split("meta-llama/llama-3", known),
            (None, "meta-llama/llama-3")
        );
        assert_eq!(split("qwen", known), (None, "qwen"));
        assert_eq!(split("ollama/", known), (None, "ollama/"));
    }
}

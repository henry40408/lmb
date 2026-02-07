super::define_codec_binding!(
    YamlBinding,
    "yaml",
    serde_yaml::from_str::<serde_yaml::Value>,
    serde_yaml::to_string
);

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_yaml_encode_decode() {
        let source = include_str!("../fixtures/bindings/codecs/yaml.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}

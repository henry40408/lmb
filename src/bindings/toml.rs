super::define_codec_binding!(
    TomlBinding,
    "toml",
    toml::from_str::<toml::Value>,
    toml::to_string
);

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_toml_encode_decode() {
        let source = include_str!("../fixtures/bindings/codecs/toml.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}

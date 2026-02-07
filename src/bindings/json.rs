use serde_json::Value;

super::define_codec_binding!(
    JsonBinding,
    "json",
    serde_json::from_str::<Value>,
    serde_json::to_string
);

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_json_binding() {
        let source = include_str!("../fixtures/bindings/codecs/json.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}

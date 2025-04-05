use oci_spec::{
    image::ImageConfiguration,
    runtime::{ProcessBuilder, RootBuilder, Spec, SpecBuilder},
    OciSpecError,
};

pub fn create_runtime_spec(config: ImageConfiguration) -> Result<Spec, OciSpecError> {
    if config.config().is_none() {
        return Ok(Spec::default());
    }
    let config = config.config().as_ref().unwrap();
    let spec = SpecBuilder::default();

    let mut process = ProcessBuilder::default().terminal(true);
    if let Some(args) = config.cmd() {
        process = process.args(args.clone());
    }

    if let Some(env) = config.env() {
        process = process.env(env.clone());
    }

    let root = RootBuilder::default()
        .path("rootfs")
        .readonly(false)
        .build()?;

    spec.process(process.build()?)
        .root(root)
        .hostname("node")
        .build()
}

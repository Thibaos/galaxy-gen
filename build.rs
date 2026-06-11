use spirv_builder::SpirvBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell cargo to re-run this build script when the shader sources change.
    println!("cargo:rerun-if-changed=galaxy-shader/src/");
    println!("cargo:rerun-if-changed=galaxy-shader/Cargo.toml");

    let mut builder = SpirvBuilder::new("galaxy-shader", "spirv-unknown-vulkan1.2");
    builder.build_script.env_shader_spv_path = Some(true);
    builder.build()?;
    Ok(())
}

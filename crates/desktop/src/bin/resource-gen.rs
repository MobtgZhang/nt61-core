use nt61_desktop::resource_generator::generate_desktop_resources;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let mut output = PathBuf::from("build/desktop-resources");
    let mut arch = String::from("x86_64");
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--output" => {
                output = arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| String::from("--output requires a directory path"))?;
            }
            "--arch" => {
                arch = arguments
                    .next()
                    .ok_or_else(|| String::from("--arch requires an architecture"))?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let written = generate_desktop_resources(&output, &arch)
        .map_err(|error| format!("resource generation failed: {error}"))?;
    for path in written {
        println!("{}", path.display());
    }
    Ok(())
}

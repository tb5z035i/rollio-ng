use std::fs;

fn main() {
    let base_url = std::env::var("DATALOOP_BASE_URL").expect("set DATALOOP_BASE_URL env var");
    let token = std::env::var("DATALOOP_TOKEN").expect("set DATALOOP_TOKEN env var");
    let project_id = std::env::var("PROJECT_ID").expect("set PROJECT_ID env var");

    println!("Connecting to Dataloop at {base_url}");
    println!("Project ID: {project_id}");

    let client = rollio_storage_dataloop::ffi::DataloopClient::new(&base_url, &token)
        .expect("failed to create DataloopClient");
    println!("Client created successfully");

    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let test_file = tmp_dir.path().join("test_episode.txt");
    fs::write(
        &test_file,
        "hello from rollio-storage-dataloop smoke test\n",
    )
    .expect("failed to write test file");

    let tags_json = r#"{"episode_index":"0","source":"rollio-smoke-test"}"#;
    println!("Uploading test file: {}", test_file.display());

    match client.create_episode(&test_file, &project_id, tags_json) {
        Ok(result) => {
            println!("SUCCESS: episode created");
            println!("  episode_id: {}", result.episode_id);
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}

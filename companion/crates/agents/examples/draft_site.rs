//! Drafts site keys from a saved page inventory (an extension `inventory` message as JSON), with
//! the AI provider in ~/.config/openpager/agent.toml:
//! `cargo run -p openpager-agents --example draft_site -- inventory.json en.wikipedia.org /wiki/Main_Page`
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let message: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("read")).expect("json");
    let items = message["items"].as_array().cloned().unwrap_or_default();
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    let config = openpager_agents::AgentConfig::load(&home.join(".config/openpager/agent.toml")).expect("agent.toml");
    let started = std::time::Instant::now();
    match openpager_agents::site::draft_site_library(&config, &args[2], &args[3], "Linux", &openpager_agents::site::Scope::Site, &items).await {
        Ok(draft) => println!("{} controls -> {} keys in {:.1} s, {} attempt(s)\n{}", items.len(), draft.library.keys.len(),
                              started.elapsed().as_secs_f32(), draft.attempts, draft.toml),
        Err(e) => println!("failed: {e}"),
    }
}

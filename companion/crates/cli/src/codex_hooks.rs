//! Codex lifecycle adapter. No transcript, prompt, cwd or answer text is relayed.
use std::{io::Read,path::{Path,PathBuf},hash::{Hash,Hasher}};
use serde_json::{json,Value};
static LAST_EVENT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static PERMISSIONS:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static COMPLETIONS:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
pub fn observed(event:&Value){use std::sync::atomic::Ordering::Relaxed;let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();LAST_EVENT.store(now,Relaxed);if event["hook_event_name"]=="PermissionRequest"{PERMISSIONS.fetch_add(1,Relaxed);}if event["hook_event_name"]=="Stop"{COMPLETIONS.fetch_add(1,Relaxed);}}
const EVENTS:[(&str,&str,u32);6]=[("PermissionRequest","permission",310),("PostToolUse","event",5),("UserPromptSubmit","event",5),("Stop","event",5),("Interrupt","event",3),("SessionEnd","event",3)];
fn home()->PathBuf{std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(||std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default().join(".codex"))}
fn path()->PathBuf{home().join("hooks.json")}
pub fn enabled()->bool{std::fs::read_to_string(knoend_transport::pairing::config_dir().join("codex-agent.json")).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok()).and_then(|v|v["enabled"].as_bool()).unwrap_or(true)}
pub fn configure(enabled:bool)->Result<(),String>{let dir=knoend_transport::pairing::config_dir();std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;std::fs::write(dir.join("codex-agent.json"),json!({"enabled":enabled}).to_string()).map_err(|e|e.to_string())}
fn ours(handler:&Value)->bool{handler["command"].as_str().is_some_and(|s|s.contains(" codex-hook "))}
fn remove_ours(settings:&mut Value){if let Some(hooks)=settings.get_mut("hooks").and_then(Value::as_object_mut){for groups in hooks.values_mut(){if let Some(groups)=groups.as_array_mut(){for group in groups.iter_mut(){if let Some(handlers)=group["hooks"].as_array_mut(){handlers.retain(|h|!ours(h));}}groups.retain(|g|!g["hooks"].as_array().is_some_and(Vec::is_empty));}}}}
fn command(exe:&str,endpoint:&str,kind:&str,windows:bool)->Result<String,String>{
 let quote=|p:&str|->Result<String,String>{if windows {if !p.chars().all(|c|c.is_ascii_alphanumeric()||"\\/:._-".contains(c)){return Err("Choose a companion runtime path without spaces or shell metacharacters for native Windows hooks".into())}Ok(p.into())}else{Ok(format!("'{}'",p.replace('\'',"'\"'\"'")))}};
 Ok(format!("{} codex-hook {kind} --endpoint-file {}",quote(exe)?,quote(endpoint)?))
}
fn with_hooks(mut settings:Value,exe:&str,endpoint:&str)->Result<Value,String>{
 if !settings.is_object(){return Err("Codex hooks file must be an object; not changed".into())}
 if settings.get("hooks").is_some_and(|v|!v.is_object()){return Err("Codex hooks must be an object; not changed".into())}
 for (event,_,_) in EVENTS {if settings["hooks"].get(event).is_some_and(|v|!v.is_array()){return Err("Invalid existing Codex hook group; not changed".into())}}
 remove_ours(&mut settings);
 let hooks=settings.as_object_mut().unwrap().entry("hooks").or_insert_with(||json!({})).as_object_mut().unwrap();
 for (event,kind,timeout) in EVENTS {
  hooks.entry(event).or_insert_with(||json!([])).as_array_mut().unwrap().push(json!({"hooks":[{"type":"command","command":command(exe,endpoint,kind,cfg!(windows))?,"timeout":timeout}]}));
 }Ok(settings)
}
fn read_settings()->Result<Value,String>{match std::fs::read_to_string(path()){Ok(s)=>serde_json::from_str(&s).map_err(|_|"Invalid Codex hooks JSON; not changed".into()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(json!({})),Err(_)=>Err("Cannot read Codex hooks file; not changed".into())}}
fn save(settings:&Value)->Result<(),String>{
 let p=path();std::fs::create_dir_all(p.parent().unwrap()).map_err(|e|e.to_string())?;
 let backup=p.with_extension("json.before-knoend-codex");if !backup.exists(){if p.exists(){std::fs::copy(&p,&backup).map_err(|e|e.to_string())?;}else{std::fs::write(&backup,"{}\n").map_err(|e|e.to_string())?;}}
 let temp=p.with_extension(format!("json.{}.tmp",knoend_transport::auth::nonce()));
 use std::io::Write;let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|e|e.to_string())?;
 let result=(||{file.write_all((serde_json::to_string_pretty(settings).unwrap()+"\n").as_bytes())?;file.sync_all()?;drop(file);std::fs::rename(&temp,&p)})();
 if result.is_err(){let _=std::fs::remove_file(&temp);}result.map_err(|e:std::io::Error|e.to_string())
}
pub fn install()->Result<String,String>{let exe=crate::browser::clean_exe_path().map_err(|e|e.to_string())?;let settings=with_hooks(read_settings()?,exe.to_str().ok_or("Executable path is not UTF-8")?,crate::agent_hooks::socket_path().to_str().ok_or("Endpoint path is not UTF-8")?)?;save(&settings)?;Ok("Native Codex hooks prepared. In Codex, open /hooks and review/trust the KnoEnd hooks. Restart or open a new local session afterwards. Existing notify/config settings were preserved.".into())}
pub fn uninstall()->Result<String,String>{let mut settings=read_settings()?;remove_ours(&mut settings);save(&settings)?;Ok("KnoEnd's Codex hooks removed; other hooks and notify settings preserved.".into())}
pub fn diagnostics()->Value{let settings=read_settings();let installed=settings.as_ref().ok().is_some_and(|v|EVENTS.iter().all(|(event,_,_)|v["hooks"][event].as_array().is_some_and(|g|g.iter().any(|g|g["hooks"].as_array().is_some_and(|h|h.iter().any(ours))))));json!({"enabled":enabled(),"installed":installed,"events_received":LAST_EVENT.load(std::sync::atomic::Ordering::Relaxed)>0,"permission_events":PERMISSIONS.load(std::sync::atomic::Ordering::Relaxed),"completion_events":COMPLETIONS.load(std::sync::atomic::Ordering::Relaxed),"config_readable":settings.is_ok(),"trust_status":"Review/trust exact definitions in Codex /hooks; not inferred from file presence","events":EVENTS.iter().map(|e|e.0).collect::<Vec<_>>(),"completion_requires_pager_enabled":true,"question_answers":"answer ordinary Codex questions in the app/terminal","permission_scope":"allow or deny one request; no always/stop/updated input"})}
pub fn normalize(kind:&str,input:&Value)->Option<Value>{
 let event=input["hook_event_name"].as_str()?;
 if !EVENTS.iter().any(|(name,k,_)|*name==event&&*k==kind){return None}
 let session=input["session_id"].as_str().filter(|s|!s.is_empty()&&s.len()<=160)?;
 let mut out=json!({"knoend_agent":"codex","session_id":session,"hook_event_name":event});
 if let Some(turn)=input["turn_id"].as_str().filter(|s|s.len()<=160){out["turn_id"]=json!(turn);}
 if event=="PermissionRequest"||event=="PostToolUse" {
  let tool=input["tool_name"].as_str().filter(|s|!s.is_empty()&&s.len()<=100)?;
  out["tool_name"]=json!(tool);
  fn canonical(value:&Value)->Value{match value{Value::Object(map)=>{let sorted:std::collections::BTreeMap<_,_>=map.iter().map(|(k,v)|(k.clone(),canonical(v))).collect();serde_json::to_value(sorted).unwrap()},Value::Array(values)=>Value::Array(values.iter().map(canonical).collect()),v=>v.clone()}}
  let mut hash=std::collections::hash_map::DefaultHasher::new();canonical(&input["tool_input"]).to_string().hash(&mut hash);out["knoend_tool_fingerprint"]=json!(format!("{:x}",hash.finish()));
  if event=="PostToolUse"{return Some(out)}
  out["tool_input"]=input["tool_input"].clone();
  let detail=crate::agent_hooks::detail(&out);let fits=input["tool_input"].is_object()&&detail.is_ascii()&&detail.len()<=220;
  out["knoend_allow"]=json!(fits);
 }
 Some(out)
}
pub fn run(kind:&str,endpoint:Option<&Path>)->Result<(),String>{
 let mut bytes=Vec::new();std::io::stdin().take(65537).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
 if bytes.len()>65536{return Ok(())}
 let Some(mut event)=serde_json::from_slice::<Value>(&bytes).ok().and_then(|v|normalize(kind,&v)) else{return Ok(())};
 if std::env::var_os("WT_SESSION").is_some()||std::env::var_os("TERM_PROGRAM").is_some(){event["knoend_host"]=json!("terminal");}
 if let Some(p)=endpoint{crate::agent_hooks::relay_at(kind,event,p)?;}else{crate::agent_hooks::relay(kind,event)?;}
 Ok(())
}
pub fn completion(event:&Value)->Option<crate::pager::Notice>{
 if !enabled()||event["hook_event_name"]!="Stop"{return None}
 let mut session=std::collections::hash_map::DefaultHasher::new();event["session_id"].as_str()?.hash(&mut session);let mut h=std::collections::hash_map::DefaultHasher::new();event["turn_id"].as_str()?.hash(&mut h);
 let host=if event["knoend_host"]=="terminal"||event["knoend_tty"].is_string(){"terminal"}else{"desktop"};
 Some(crate::pager::Notice{id:format!("codex-{host}-complete-{:x}-{:x}",session.finish(),h.finish()),app:"codex".into(),kind:"completion".into(),summary:"Codex finished a turn".into(),body:if host=="terminal"{"Open the terminal to read the result, or dismiss this alert."}else{"Open Codex to read the result, or dismiss this alert."}.into()})
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn lifecycle_payload_drops_private_context_and_unknown_events(){let raw=json!({"hook_event_name":"Stop","session_id":"synthetic-session","turn_id":"t1","cwd":"private","transcript_path":"private","last_assistant_message":"private","prompt":"private"});let n=normalize("event",&raw).unwrap();assert!(!n.to_string().contains("private"));assert!(normalize("permission",&raw).is_none());assert_eq!(completion(&n).unwrap().kind,"completion");}
 #[test]fn post_tool_events_use_fingerprints_without_transferring_inputs(){
  let mut raw=json!({"hook_event_name":"PermissionRequest","session_id":"s","turn_id":"t","tool_name":"Bash","tool_input":{"command":"echo private-input","description":"test"}});
  let request=normalize("permission",&raw).unwrap();raw["hook_event_name"]=json!("PostToolUse");let event=normalize("event",&raw).unwrap();
  assert!(!event.to_string().contains("private-input"));assert!(event.get("tool_input").is_none());assert!(crate::agent_hooks::settles(&event,&request));
  raw["tool_input"]["command"]=json!("different");assert!(!crate::agent_hooks::settles(&normalize("event",&raw).unwrap(),&request));
 }
 #[test]fn first_install_accepts_no_hook_file_and_removal_keeps_it_empty(){
  let mut empty=json!({});remove_ours(&mut empty);assert_eq!(empty,json!({}));
  let settings=with_hooks(empty,"C:/tools/knoend.exe","C:/local/agents.sock").unwrap();
  assert_eq!(settings["hooks"].as_object().unwrap().len(),6);
 }
 #[test]fn installation_is_idempotent_preserves_other_handlers_and_notify(){let settings=json!({"notify":["existing"],"hooks":{"Stop":[{"hooks":[{"type":"command","command":"existing"}]}]}});let one=with_hooks(settings,"C:/tools/knoend.exe","C:/local/agents.sock").unwrap();let two=with_hooks(one.clone(),"C:/tools/knoend.exe","C:/local/agents.sock").unwrap();assert_eq!(one,two);let mut removed=two;remove_ours(&mut removed);assert_eq!(removed["hooks"]["Stop"][0]["hooks"][0]["command"],"existing");assert_eq!(removed["notify"][0],"existing");}
 #[test]fn long_or_non_ascii_requests_cannot_be_approved_blindly(){for command in ["x".repeat(500),"é".into()]{let n=normalize("permission",&json!({"session_id":"s","turn_id":"t","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":command}})).unwrap();assert_eq!(n["knoend_allow"],false);}}
}

//! Recipe-owned presentation modes. No application key labels or slot maps live here.
use std::{collections::HashMap,sync::Mutex,hash::{Hash,Hasher}};
use knoend_core::{library::{Action,Library,PresentationMode},protocol::{Grid,LayoutBody,Send}};
#[derive(Clone)]pub struct Rendered {pub layout:String,pub touchpad:bool,pub gain:f64,pub actions:HashMap<u8,Action>}
#[derive(Default)]struct State {library:Option<Library>,mode:Option<String>,rendered:Option<Rendered>}
static STATE:Mutex<State>=Mutex::new(State{library:None,mode:None,rendered:None});
pub fn configure(library:Library){let mut state=STATE.lock().unwrap();if state.library.as_ref().is_some_and(|l|l.pack.id!=library.pack.id){state.mode=None;state.rendered=None;}state.library=Some(library);}
pub fn reset(){let mut state=STATE.lock().unwrap();state.mode=None;state.rendered=None;}
pub fn mode()->String{let state=STATE.lock().unwrap();state.mode.clone().or_else(||state.library.as_ref().and_then(|l|l.presentation.as_ref()).map(|s|s.initial_mode.clone())).unwrap_or_default()}
pub fn set_mode(mode:&str)->Result<PresentationMode,String>{let mut state=STATE.lock().unwrap();let target=state.library.as_ref().and_then(|l|l.presentation.as_ref()).and_then(|s|s.modes.get(mode)).cloned().ok_or("Unknown presentation mode")?;state.mode=Some(mode.into());state.rendered=None;Ok(target)}
pub fn target(mode:&str)->Option<PresentationMode>{STATE.lock().unwrap().library.as_ref()?.presentation.as_ref()?.modes.get(mode).cloned()}
pub fn shortcut(command:&str)->Option<String>{STATE.lock().unwrap().library.as_ref()?.presentation.as_ref()?.commands.get(command).cloned()}
pub fn current()->Option<Rendered>{STATE.lock().unwrap().rendered.clone()}
pub fn action(layout:&str,key:u8)->Option<Action>{let state=STATE.lock().unwrap();let r=state.rendered.as_ref()?;if r.layout!=layout{return None}r.actions.get(&key).cloned()}
pub fn render(library:&Library,requested:&str,max:Grid,capable:bool,notes:&str)->Result<(LayoutBody,Rendered),String>{
 let spec=library.presentation.as_ref().ok_or("No presentation configuration")?;
 let mut selected=if spec.modes.contains_key(requested){requested}else{&spec.initial_mode};
 for _ in 0..=spec.modes.len(){
  let mode=spec.modes.get(selected).ok_or("Missing presentation mode")?;let grid=Grid::parse(&mode.grid).ok_or("Invalid presentation grid")?;
  if (mode.touchpad&&!capable)||!grid.fits(max){selected=mode.fallback.as_deref().ok_or("This mode needs a compatible grid/touchpad board; configure a fallback mode")?;continue}
  let mut hash=std::collections::hash_map::DefaultHasher::new();serde_json::to_string(spec).unwrap_or_default().hash(&mut hash);selected.hash(&mut hash);
  let id=format!("system.presentation.{:x}.{selected}",hash.finish());
  let mut mode_library=library.clone();mode_library.presentation=None;mode_library.pack.id=id.clone();mode_library.pack.grid=Some(mode.grid.clone());mode_library.pages.clear();mode_library.keys=mode.keys.clone();
  let pages=knoend_core::layout::pages(&mode_library,max);if pages.len()!=1{return Err("Presentation modes must fit one configured page".into())}
  let mut body=knoend_core::layout::build(&mode_library,knoend_core::focus::Os::current(),&pages,0).map_err(|e|e.to_string())?;
  for key in &mut body.keys{key.send=Send::Event(true)}
  body.touchpad=mode.touchpad.then_some(true);body.detail=mode.notes.then(||notes.to_string());
  let actions=pages[0].keys.iter().map(|(i,slot)|(*slot,mode_library.keys[*i].action.clone())).collect();
  let rendered=Rendered{layout:id,touchpad:mode.touchpad,gain:spec.pointer_gain,actions};return Ok((body,rendered));
 }Err("Presentation fallback cycle".into())
}
pub fn screen(max:Grid,capable:bool,notes:&str)->Result<LayoutBody,String>{
 let mut state=STATE.lock().unwrap();let library=state.library.as_ref().ok_or("No active presenter recipe")?;
 let requested=state.mode.as_deref().unwrap_or(&library.presentation.as_ref().ok_or("No presenter modes")?.initial_mode);
 let (body,rendered)=render(library,requested,max,capable,notes)?;state.rendered=Some(rendered);Ok(body)
}
pub fn diagnostics()->serde_json::Value{let state=STATE.lock().unwrap();serde_json::json!({"recipe":state.library.as_ref().map(|l|&l.pack.id),"mode":state.mode,"layout":state.rendered.as_ref().map(|r|&r.layout),"pointer_gain":state.rendered.as_ref().map(|r|r.gain)})}
#[cfg(test)]mod tests{use super::*;
 #[test]fn invalid_recipe_modes_are_rejected_before_reload(){
  let text=include_str!("../../../../recipes/apps/powerpnt.toml");
  let mut library=Library::parse(text).unwrap();
  library.presentation.as_mut().unwrap().modes.get_mut("pointer-keys").unwrap().fallback=Some("pointer".into());
  assert!(Library::parse(&toml::to_string(&library).unwrap()).unwrap_err().to_string().contains("fallback cycle"));
  let mut library=Library::parse(text).unwrap();library.presentation.as_mut().unwrap().commands.remove("next");
  assert!(Library::parse(&toml::to_string(&library).unwrap()).unwrap_err().to_string().contains("missing command shortcut next"));
 }
 #[test]fn recipe_labels_slots_and_touchpad_fallback_are_data(){
  let text=include_str!("../../../../recipes/apps/powerpnt.toml");let mut library=Library::parse(text).unwrap();
  let (body,_)=render(&library,"pointer",Grid::DEFAULT,true,"").unwrap();assert_eq!(body.touchpad,Some(true));assert_eq!(body.keys.len(),3);
  let (body,_)=render(&library,"pointer",Grid::DEFAULT,false,"").unwrap();assert_eq!(body.touchpad,None);assert_eq!(body.keys.len(),9);
  let mode=library.presentation.as_mut().unwrap().modes.get_mut("controls").unwrap();mode.keys[0].label="Earlier".into();mode.keys[0].slot=Some(9);mode.keys[8].slot=Some(1);
  let(body,r)=render(&library,"controls",Grid::DEFAULT,true,"").unwrap();assert_eq!(body.keys.iter().find(|k|k.key==9).unwrap().label,"Earlier");assert!(matches!(r.actions[&9],Action::Presentation{..}));
 }
}

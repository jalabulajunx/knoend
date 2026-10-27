//! Native Windows adapters. Runs in a supervised child of this same executable;
//! blocking COM/UIA providers cannot stall the companion or require PowerShell.
use std::{collections::{HashMap,HashSet},hash::{Hash,Hasher},time::{Duration,SystemTime,UNIX_EPOCH}};
use serde_json::{json,Value};
use windows::{core::{w,BSTR,GUID,Interface,PCWSTR,BOOL},Win32::{Foundation::{CloseHandle,HWND,LPARAM,RECT,WAIT_TIMEOUT},System::{Com::*,Ole::GetActiveObject,Threading::*,Variant::VARIANT},UI::{Accessibility::*,WindowsAndMessaging::*}}};
use crate::powerpoint::LiveSlide;

struct Apartment;
impl Apartment { fn new(kind: COINIT) -> windows::core::Result<Self> { unsafe{CoInitializeEx(None,kind).ok()?;} Ok(Self) } }
impl Drop for Apartment { fn drop(&mut self){unsafe{CoUninitialize()}} }
struct Dispatch(IDispatch);
impl Dispatch {
 fn value(&self,name:&str,args:&[VARIANT])->windows::core::Result<VARIANT>{unsafe{
  let name:Vec<u16>=name.encode_utf16().chain(Some(0)).collect();let name=PCWSTR(name.as_ptr());let mut id=0;
  self.0.GetIDsOfNames(&GUID::zeroed(),&name,1,0x400,&mut id)?;
  let mut args:Vec<VARIANT>=args.iter().rev().cloned().collect();
  let params=DISPPARAMS{rgvarg:args.as_mut_ptr(),cArgs:args.len() as u32,..Default::default()};
  let mut result=VARIANT::default();
  self.0.Invoke(id,&GUID::zeroed(),0x400,if args.is_empty(){DISPATCH_PROPERTYGET}else{DISPATCH_PROPERTYGET|DISPATCH_METHOD},&params,Some(&mut result),None,None)?;
  Ok(result)
 }}
 fn object(&self,name:&str)->windows::core::Result<Self>{Ok(Self(IDispatch::try_from(&self.value(name,&[])?)?))}
 fn item(&self,index:i32)->windows::core::Result<Self>{Ok(Self(IDispatch::try_from(&self.value("Item",&[index.into()])?)?))}
 fn int(&self,name:&str)->windows::core::Result<i32>{i32::try_from(&self.value(name,&[])?) }
 fn text(&self,name:&str)->windows::core::Result<String>{Ok(BSTR::try_from(&self.value(name,&[])?)?.to_string())}
}
fn powerpoint()->windows::core::Result<LiveSlide>{unsafe{
 let cls=CLSIDFromProgID(w!("PowerPoint.Application"))?;let mut unknown=None;GetActiveObject(&cls,None,&mut unknown)?;
 let app=Dispatch(unknown.ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005u32 as i32)))?.cast()?);
 let shows=app.object("SlideShowWindows")?;
 let mut state=LiveSlide{available:true,..Default::default()};
 if shows.int("Count")?==0{return Ok(state)}
 let show=shows.item(1)?;let pres=show.object("Presentation")?;let slide=show.object("View")?.object("Slide")?;
 state.presenting=true;state.name=pres.text("Name")?;state.path=pres.text("FullName").unwrap_or_default();
 state.slide=slide.int("SlideIndex")?.max(0) as u32;state.total=pres.object("Slides")?.int("Count")?.max(0) as u32;
 let notes=(||->windows::core::Result<String>{
  let shapes=slide.object("NotesPage")?.object("Shapes")?;let mut notes=Vec::new();
  for i in 1..=shapes.int("Count")?.min(100){let shape=shapes.item(i)?;
   if shape.int("Type")?==14 && shape.object("PlaceholderFormat")?.int("Type")?==2 && shape.int("HasTextFrame")?!=0 {
    notes.push(shape.object("TextFrame")?.object("TextRange")?.text("Text")?);
   }
  }Ok(notes.join("\n").trim().chars().take(60000).collect())
 })();
 state.notes_available=notes.is_ok();state.notes=notes.unwrap_or_default();Ok(state)
}}

#[derive(Default)]struct Window {hwnd:isize,process:String,class:String,name:String,width:i32,height:i32,style:u32}
fn notification_windows()->Vec<Window>{
 let mut result:Vec<Window>=Vec::new();
 unsafe extern "system" fn visit(h:HWND,l:LPARAM)->BOOL{
  let list=&mut *(l.0 as *mut Vec<Window>);if !IsWindowVisible(h).as_bool(){return BOOL(1)}
  let mut pid=0;GetWindowThreadProcessId(h,Some(&mut pid));
  let Some(exe)=knoend_focus::process_exe(pid) else{return BOOL(1)};
  let process=exe.file_stem().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
  if !["ms-teams","teams","shellexperiencehost","shellhost","explorer","applicationframehost"].contains(&process.as_str()){return BOOL(1)}
  let mut c=[0u16;256];let n=GetClassNameW(h,&mut c);let class=String::from_utf16_lossy(&c[..n.max(0) as usize]);
  let mut t=[0u16;1024];let n=GetWindowTextW(h,&mut t);let name=String::from_utf16_lossy(&t[..n.max(0) as usize]);
  let mut r=RECT::default();if GetWindowRect(h,&mut r).is_err(){return BOOL(1)}
  list.push(Window{hwnd:h.0 as isize,process,class,name,width:r.right-r.left,height:r.bottom-r.top,style:GetWindowLongW(h,GWL_EXSTYLE) as u32});BOOL(1)
 }
 unsafe{let _=EnumWindows(Some(visit),LPARAM(&mut result as *mut _ as isize));}result
}
fn candidate(w:&Window)->Option<&'static str>{
 let name=w.name.to_lowercase();let class=w.class.to_lowercase();
 if name.contains("notification center")||name.contains("notification centre")||name.contains("action center")||name.contains("centre de notification") {return None}
 if ["CabinetWClass","ExploreWClass","Progman","WorkerW","Shell_TrayWnd","Shell_SecondaryTrayWnd"].contains(&w.class.as_str()){return None}
 let named=name.starts_with("new notification")||name.starts_with("nouvelle notification")||name.starts_with("incoming call")||name.starts_with("appel entrant")||name.starts_with("microsoft teams notification")||name.starts_with("teams notification");
 let popup=class.contains("notification")||class.contains("toast");
 if ["ms-teams","teams"].contains(&w.process.as_str()) {
  if (w.class=="TeamsWebView"&&w.style&0x88==0x88&&w.width>=150&&w.width<1000&&w.height>=50&&w.height<650)||popup||(named&&w.width<900&&w.height<650){return Some("teams")}
 }else if (named||popup)&&w.width>0&&w.height>0&&w.width<1000&&w.height<900{return Some("system")}
 None
}
fn notice_app(names:&[String])->Option<&'static str>{
 let brand=|s:&str|match s.trim(){"Microsoft Teams"|"Microsoft Teams (work or school)"|"Teams"=>Some("teams"),"ChatGPT"=>Some("chatgpt"),"Codex"=>Some("codex"),_=>None};
 for s in names {for prefix in ["New notification from ","Nouvelle notification de "] {if let Some(rest)=s.strip_prefix(prefix){
  for (name,app) in [("Microsoft Teams","teams"),("Teams","teams"),("ChatGPT","chatgpt"),("Codex","codex")] {
   if rest==name||rest.strip_prefix(name).is_some_and(|x|x.starts_with([' ', ',', '.', ':'])){return Some(app)}
  }return None
 }}}
 names.iter().find_map(|n|brand(n))
}
fn incoming(buttons:&[String])->bool{
 let accept=buttons.iter().any(|s|["Accept call","Accept audio call","Accept video call","Accept with audio","Accept with video","Answer","Answer call","Answer audio call","Answer video call","Accepter","Accepter l'appel","Accepter audio","Accepter vidéo"].iter().any(|x|s.eq_ignore_ascii_case(x)));
 let decline=buttons.iter().any(|s|["Decline","Decline call","Refuser","Refuser l'appel"].iter().any(|x|s.eq_ignore_ascii_case(x)));accept&&decline
}
fn emit(value:Value){println!("{value}")}
fn parent_alive()->bool{
 let Some(pid)=std::env::var("KNOEND_PARENT_PID").ok().and_then(|v|v.parse::<u32>().ok()) else{return true};
 unsafe{let Ok(h)=OpenProcess(PROCESS_SYNCHRONIZE,false,pid) else{return false};let alive=WaitForSingleObject(h,0)==WAIT_TIMEOUT;let _=CloseHandle(h);alive}
}
pub fn run(kind:&str,once:bool)->Result<(),String>{
 let _desktop=knoend_focus::Watcher::connect()?;
 let _apartment=Apartment::new(if kind=="powerpoint"{COINIT_APARTMENTTHREADED}else{COINIT_MULTITHREADED}).map_err(|e|e.to_string())?;
 if kind=="powerpoint" {
  while parent_alive(){let state=powerpoint().unwrap_or_default();println!("{}",serde_json::to_string(&state).unwrap());if once{break}std::thread::sleep(Duration::from_millis(500))}return Ok(())
 }
 if kind!="pager"{return Err("Unknown native adapter".into())}
 let automation:IUIAutomation=unsafe{CoCreateInstance(&CUIAutomation,None,CLSCTX_INPROC_SERVER)}.map_err(|e|e.to_string())?;
 let condition=unsafe{automation.CreateTrueCondition()}.map_err(|e|e.to_string())?;
 let mut visible_ids:HashMap<isize,(String,String,String)>=HashMap::new();let mut seen:HashMap<String,std::time::Instant>=HashMap::new();
 let epoch=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();let mut generation=0;let mut pass=0;
 emit(json!({"health":"ready"}));
 while parent_alive(){
  let windows=notification_windows();let roots=windows.len();let(mut candidates,mut matched,mut errors)=(0,0,0);let mut visible=HashSet::new();
  if crate::pager::config().enabled {
   for w in windows {let Some(class)=candidate(&w) else{continue};candidates+=1;
    let teams=class=="teams";
    if teams{matched+=1;visible.insert(w.hwnd);
     visible_ids.entry(w.hwnd).or_insert_with(||{generation+=1;let id=format!("teams-{}-{epoch}-{generation}",w.hwnd);emit(json!({"id":id,"app":"teams","kind":"message","summary":"Teams notification","body":""}));(id,"message".into(),String::new())});
    }
    let content=(||->windows::core::Result<(Vec<String>,Vec<String>,Vec<String>)>{unsafe{
     let root=automation.ElementFromHandle(HWND(w.hwnd as *mut _))?;let mut names=vec![w.name.clone(),root.CurrentName()?.to_string()];let mut buttons=Vec::new();let mut text=Vec::new();
     let all=root.FindAll(TreeScope_Descendants,&condition)?;
     for i in 0..all.Length()?.min(160){let item=all.GetElement(i)?;let name=item.CurrentName()?.to_string();if name.is_empty(){continue}if names.len()<62{names.push(name.clone())}
      let control=item.CurrentControlType()?;if control==UIA_ButtonControlTypeId{buttons.push(name.clone())}if control==UIA_TextControlTypeId&&text.len()<14{text.push(name)}
     }Ok((names,buttons,text))
    }})();
    let Ok((names,buttons,text))=content else{errors+=1;continue};
    let Some(app)=(if teams{Some("teams")}else{notice_app(&names)})else{continue};if !teams{matched+=1}
    let kind=if app=="teams"&&incoming(&buttons){"call"}else{"message"};
    let body:String=text.into_iter().filter(|s|!["Microsoft Teams","Teams","ChatGPT","Codex"].contains(&s.as_str())).collect::<Vec<_>>().join(" ").chars().take(2000).collect();
    let summary=if kind=="call"{"Incoming Teams call"}else{"Notification"};
    if teams {let (id,old_kind,old_body)=visible_ids.get_mut(&w.hwnd).unwrap();
     if old_kind!=kind||old_body!=&body {emit(json!({"id":id,"app":app,"kind":kind,"summary":summary,"body":body}));*old_kind=kind.into();*old_body=body;}
    }else{
     if kind=="call" {visible.insert(w.hwnd);}
     let mut hash=std::collections::hash_map::DefaultHasher::new();(app,w.hwnd,&body).hash(&mut hash);let id=format!("toast-{:x}",hash.finish());
     if !seen.contains_key(&id){emit(json!({"id":id,"app":app,"kind":kind,"summary":summary,"body":body}));seen.insert(id.clone(),std::time::Instant::now());}
     if kind=="call" {visible_ids.insert(w.hwnd,(id,"call".into(),body));}
    }
   }
  }
  visible_ids.retain(|hwnd,(id,kind,_)|{if visible.contains(hwnd){true}else{if kind=="call"{emit(json!({"id":id,"app":"teams","kind":"clear","summary":"","body":""}))}false}});
  seen.retain(|_,t|t.elapsed().as_secs()<600);pass+=1;emit(json!({"health":"scan","pass":pass,"roots":roots,"candidates":candidates,"matched":matched,"errors":errors}));
  if once{break}std::thread::sleep(Duration::from_millis(750));
 }Ok(())
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn observed_banner_and_attribution_regressions(){
  let w=Window{process:"ms-teams".into(),class:"TeamsWebView".into(),width:372,height:136,style:0x200088,..Default::default()};assert_eq!(candidate(&w),Some("teams"));assert_eq!(candidate(&Window{style:0x200000,..w}),None);
  assert_eq!(notice_app(&["New notification from Outlook".into(),"Teams".into()]),None);
  assert_eq!(notice_app(&["New notification from Microsoft Teams, example".into()]),Some("teams"));
  assert!(incoming(&["Accept audio call".into(),"Decline call".into()]));assert!(!incoming(&["Leave".into(),"Mute".into()]));
 }
}

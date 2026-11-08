use super::*;
use crate::hosted::{Execution, Snapshot};
use crate::runner::SessionRunner;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

// Real GDK worker sessions run under the hosted VFS, transport and checkpoint
// scopes. The public jobs API does not expose force, so drive its parent directly.
#[test]
fn forced_generation_resumes_after_matcher_repair_without_replaying_a_completed_group() {
    let (files, facts, criteria) = {
        let fs = crate::vfs::Scope::enter(&Snapshot::new());
        crate::vfs::create_dir_all("/project/.jazyk").unwrap();
        crate::vfs::write("/project/.jazyk/jazyk.toml",
            "version=1\n[[sets]]\nid='ds_main'\nname='Main'\ndocs=['docs/**']\nsource=['src/**','tests/**']\n").unwrap();
        let project = crate::project::Project::load(Path::new("/project"));
        let mut store = Store::load(&project.out);
        let mut facts = BTreeMap::new();
        let mut criteria = BTreeMap::new();
        for name in ["alpha", "beta"] {
            let entity = format!("ent:{name}");
            let requirement = format!("req:{name}");
            let criterion = format!("crit:{name}");
            store.graph.entities.insert(entity.clone(), crate::model::Entity { name:name.into(), ..Default::default() });
            store.graph.requirements.insert(requirement.clone(), crate::model::Requirement {
                statement:format!("The {name} component says Hello."), entities:vec![entity.clone()], ..Default::default()
            });
            store.graph.criteria.insert(criterion.clone(), crate::model::Criterion {
                requirement:requirement.clone(), statement:format!("The {name} greeting is Hello."), kind:"positive".into(), ..Default::default()
            });
            criteria.insert(name.to_string(), criteria_front_matter(&requirement,Some(&criterion),
                &store.graph.criteria[&criterion].statement) + "\nPASS only when the greeting is Hello. FAIL otherwise.\n");
            facts.insert(name.to_string(), fact_hash(&store,&entity));
        }
        store.save();
        let mut ledger = Ledger::load(&project.out);
        ledger.medium = Some(Medium { form:"text".into(), produced:"written".into(), toolchain:String::new(), artifact:String::new() });
        ledger.save(&project.out).unwrap();
        let mut control = crate::control::Control::load(&project,&project.out);
        control.generate = "auto".into(); control.save(&project.out);
        (fs.snapshot(),facts,criteria)
    };
    let config_hash = crate::model::hash_hex(std::str::from_utf8(&files[".jazyk/jazyk.toml"]).unwrap());
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let observed_requests = requests.clone();
    let model = Arc::new(move |body:String| {
        let request:Value = serde_json::from_str(&body).unwrap();
        assert_eq!(request["_jazyk"]["purpose"],"inference");
        let mut seen = observed_requests.lock().unwrap();
        seen.push(request);
        let round = seen.len();
        let name = if round <= 5 { "alpha" } else { "beta" };
        let entity = format!("ent:{name}");
        let source = if name == "alpha" { "src/alpha.txt" } else { "generated/beta.txt" };
        let record = || json!({"entity":entity,"factHash":facts[name],"manifest":{
            "files":[source],"tests":[{"requirement":format!("req:{name}"),"criterion":format!("crit:{name}"),
                "kind":"llm","artifact":format!("tests/{name}.md"),"name":name,"run":"llm","files":[source]}]}});
        let (tool,args) = match round {
            1 | 6 => ("begin_generation",json!({"entity":entity})),
            2 | 8 => ("write_text_file",json!({"path":source,"content":"Hello\n"})),
            3 | 9 => ("write_text_file",json!({"path":format!("tests/{name}.md"),"content":criteria[name]})),
            4 | 10 => ("record_generation",record()),
            5 | 11 => ("done",json!({"summary":format!("Recorded {name}.")})),
            7 => ("update_document_set",json!({"baseHash":config_hash,"set":{"id":"ds_main","source":["src/**","tests/**","generated/**"]}})),
            _ => panic!("a completed generation group was replayed"),
        };
        let reply = json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
            "id":format!("group-{round}"),"type":"function","function":{"name":tool,"arguments":args.to_string()}
        }]}}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}).to_string();
        Box::pin(async move { Ok(reply) }) as std::pin::Pin<Box<dyn std::future::Future<Output=Result<String,String>> + Send>>
    });
    let checkpoint = Arc::new(Mutex::new(None::<(Value,Snapshot)>));
    let saved = checkpoint.clone();
    let interrupted = Arc::new(AtomicBool::new(false));
    let fail = interrupted.clone();
    let sink = Arc::new(move |mut capsule:Value,files:Snapshot| {
        let next = capsule["sequence"].as_u64().unwrap() + 1;
        capsule["sequence"] = json!(next);
        let crash = capsule["frames"].as_array().and_then(|frames|frames.last()).is_some_and(|frame|
            frame["key"].as_str().is_some_and(|key|key.ends_with("g:generate:ent:beta"))
                && frame["boundary"]["phase"] == "after_tool" && frame["harness"]["rounds"] == 2)
            && !fail.swap(true,Ordering::SeqCst);
        *saved.lock().unwrap() = Some((capsule,files));
        Box::pin(async move {
            if crash { Err("checkpoint-failed: test lost acknowledgement after matcher save".into()) }
            else { Ok(next) }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output=Result<u64,String>> + Send>>
    });
    let mut execution = Execution { model:"two-group-recovery".into(), transport_async:Some(model), checkpoint:Some(sink), ..Default::default() };
    let run = |files:&Snapshot,execution:&Execution| {
        let fs = crate::vfs::Scope::enter(files);
        let _state = crate::scoped_state::Scope::enter();
        let _execution = crate::hosted::test_execution_scope(execution.clone());
        let _recovery = crate::session_recovery::Scope::enter(execution.resume.as_ref());
        let project = crate::project::Project::load(Path::new("/project"));
        let store = Store::load(&project.out);
        let guard = crate::control::begin_internal_build(&project,&project.out,"generate").unwrap();
        let llm = crate::llm::Llm { api: "chat_completions".into(), reasoning_summary: None,  base_url:"http://127.0.0.1:1/v1".into(), model:"two-group-recovery".into(), api_key:String::new(), temperature:None, trace:None };
        let runner = crate::inprocess::InProcessRunner::start(&project,&llm,&project.out).unwrap();
        runner.set_build_token(Some(guard.token().to_owned()));
        let trace = crate::session::Trace::to_sink(crate::session::TraceLevel::Quiet,Arc::new(|_|{}),Default::default());
        let result = run_selected(&store,&runner,&GenSettings::resolve(&project),&[],true,&trace,false);
        drop(guard);
        (result,fs.snapshot())
    };
    let (stopped,_) = run(&files,&execution);
    assert!(stopped.unwrap_err().contains("checkpoint-failed:"));
    assert_eq!(requests.lock().unwrap().len(),7,"the first group completed before the interruption");
    let (capsule,protected) = checkpoint.lock().unwrap().clone().unwrap();
    assert!(std::str::from_utf8(&protected[".jazyk/jazyk.toml"]).unwrap().contains("generated/**"));
    assert_eq!(protected["src/alpha.txt"],b"Hello\n");
    execution.resume = Some(capsule);
    let (complete,final_files) = run(&protected,&execution);
    let complete = complete.unwrap();
    assert_eq!(complete["regenerated"],2,"{complete}");
    assert_eq!(complete["skipped"],0,"force=true retains completed-group progress instead of recomputing skips");
    assert_eq!(complete["failures"],0,"{complete}");
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(),11,"the first group and saved matcher response must not be inferred again");
    let identities:std::collections::BTreeSet<_> = requests.iter().map(|request|request["_jazyk"]["requestId"].as_str().unwrap()).collect();
    assert_eq!(identities.len(),11,"resume must not rebill an admitted request");
    assert_eq!(final_files["src/alpha.txt"],b"Hello\n");
    assert_eq!(final_files["generated/beta.txt"],b"Hello\n");
    assert!(!final_files.keys().any(|path|path.contains("/leases/")));
}

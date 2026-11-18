//! MCP adapter for persisted changesets. The metadata lock covers one tool call,
//! not the lifetime of a session. See docs/frontends/mcp.md#recoverable-changesets.
use super::*;
use crate::open_work::{self as work, Receipt};

impl McpServer {
    pub(super) fn handle_local(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<Value, (i64, String)> {
        if method != "tools/call" {
            let mut reply = self.handle_local_inner(method, params)?;
            if method == "tools/list" {
                if let Some(list) = reply["tools"].as_array_mut() {
                    list.extend(self.open_work_tools());
                }
            }
            return Ok(reply);
        }
        if matches!(
            params["name"].as_str(),
            Some("await_changes" | "run_task" | "wait_run")
        ) {
            return self.handle_local_inner(method, params);
        }
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return self.handle_local_inner(method, params);
        }
        let _lock = work::Lock::acquire(&self.out).map_err(|e| (-32603, e))?;
        let name = params["name"].as_str().unwrap_or_default();
        let args = &params["arguments"];
        if matches!(
            name,
            "open_work" | "read_open_work" | "adopt_open_work" | "discard_open_work" | "unstage"
        ) || (name == "begin_goals" && args["workId"].is_string())
        {
            let mut result = match self.work_call(name, args) {
                Ok(v) => text_result(v, false),
                Err(e) => text_result(json!({"error":{"rule":"open-work","message":e}}), true),
            };
            let receipt = self
                .open
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|o| o.work.clone());
            if let Some(receipt) = receipt {
                if let Ok(r) = work::read(&self.out, &receipt.id) {
                    if let Some(text) = result["content"][0]["text"].as_str() {
                        if let Ok(mut value) = serde_json::from_str::<Value>(text) {
                            value["conflictNotices"] = json!(self.work_notices(&r));
                            result["content"][0]["text"] =
                                json!(serde_json::to_string_pretty(&value).unwrap());
                        }
                    }
                }
            }
            return Ok(result);
        }
        let before = self
            .open
            .lock()
            .unwrap()
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| (-32603, e.to_string()))?;
        if let Some(saved) = before.as_ref() {
            if let Ok(receipt) = serde_json::from_value::<Receipt>(saved["work"].clone()) {
                let record = work::read(&self.out, &receipt.id);
                if name == "done" {
                    if let Ok(record) = &record {
                        if record.state == "committed"
                            && record.receipt.epoch == receipt.epoch
                            && record.receipt.owner == receipt.owner
                        {
                            return Ok(text_result(record.result.clone(), false));
                        }
                    }
                }
                if let Err(e) = record.and_then(|r| r.check(&receipt)) {
                    return Ok(text_result(
                        json!({"error":{"rule":"open-work-owner","message":e}}),
                        true,
                    ));
                }
            }
        }
        drop(_lock);
        let mut reply = self.handle_local_inner(method, params)?;
        let _lock = work::Lock::acquire(&self.out).map_err(|e| (-32603, e))?;
        let saved_result = (|| -> Result<Option<Value>, String> {
            let mut guard = self.open.lock().unwrap();
            let Some(o) = guard.as_mut() else {
                return Ok(None);
            };
            let mut record = if let Some(receipt) = &o.work {
                let mut r = work::read(&self.out, &receipt.id)?;
                r.check(receipt)?;
                r.payload = serde_json::to_value(&*o).map_err(|e| e.to_string())?;
                r
            } else {
                let mut token = [0u8; 24];
                getrandom::fill(&mut token).map_err(|e| e.to_string())?;
                let token = token.iter().map(|b| format!("{b:02x}")).collect::<String>();
                let mut identity = json!({"workerId":self.worker_id(),"clientName":self.client.lock().unwrap().clone(),"session":o.session.caller.run,"batch":o.id,"pid":crate::sys::process_id(),"host":host_name(),"setId":self.project.set_id});
                if let Some(extra) = self.identity.lock().unwrap().as_object() {
                    identity.as_object_mut().unwrap().extend(extra.clone());
                }
                let r = work::create(
                    &self.out,
                    &token,
                    identity,
                    serde_json::to_value(&*o).map_err(|e| e.to_string())?,
                )?;
                o.work = Some(r.receipt.clone());
                self.work_heartbeat(o);
                r
            };
            let sequence = record.sequence;
            record.payload = serde_json::to_value(&*o).map_err(|e| e.to_string())?;
            record.last_activity = crate::sys::now_unix();
            record.expires_at = record.last_activity + crate::limits::OPEN_WORK_TTL_SECS;
            if o.heartbeat.is_none() {
                self.work_heartbeat(o);
            }
            work::update_targets(&mut record);
            work::save(&self.out, &mut record, Some(sequence))?;
            let warnings = work::warnings(&self.out, &record)?;
            let notices = self.work_notices(&record);
            Ok(Some(
                json!({"workId":record.receipt.id,"ownerToken":record.receipt.owner,"epoch":record.receipt.epoch,"warnings":warnings,"conflictNotices":notices}),
            ))
        })();
        match saved_result {
            Ok(Some(extra)) => {
                if let Some(text) = reply["content"][0]["text"].as_str() {
                    if let Ok(mut value) = serde_json::from_str::<Value>(text) {
                        if let Some(object) = value.as_object_mut() {
                            object.extend(extra.as_object().unwrap().clone());
                            reply["content"][0]["text"] =
                                json!(serde_json::to_string_pretty(&value).unwrap());
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                let restored = before.and_then(|v| serde_json::from_value(v).ok());
                *self.open.lock().unwrap() = restored;
                return Ok(text_result(
                    json!({"error":{"rule":"open-work-persistence","message":e}}),
                    true,
                ));
            }
        }
        Ok(reply)
    }

    fn open_work_tools(&self) -> Vec<Value> {
        if self.modes.len() == 1 && self.modes[0] == "runtime" {
            return Vec::new();
        }
        let mut list = vec![
            json!({"name":"open_work","description":"List persisted open graph work and expired recoverable changesets.","inputSchema":{"type":"object","properties":{"node":{"type":"string"},"goal":{"type":"string"},"state":{"type":"string","enum":["active","expired","all"]},"cursor":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100}},"additionalProperties":false}}),
            json!({"name":"read_open_work","description":"Read a persisted changeset's identity and staged mutations.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}}),
        ];
        if !self.bridge.reader
            && (self.write || self.modes.iter().any(|m| m == "compile" || m == "chat"))
        {
            for name in ["adopt_open_work", "discard_open_work"] {
                list.push(json!({"name":name,"description":"Explicitly adopt expired work, or discard own or expired work. Ownership epoch is checked atomically.","inputSchema":{"type":"object","properties":{"id":{"type":"string"},"expectedEpoch":{"type":"integer","minimum":1},"ownerToken":{"type":"string"},"reason":{"type":"string"}},"required":["id","expectedEpoch","reason"],"additionalProperties":false}}));
            }
            list.push(json!({"name":"unstage","description":"Withdraw staged mutations by zero-based mutation ID; dependent work must be repaired first.","inputSchema":{"type":"object","properties":{"mutationIds":{"type":"array","items":{"type":"integer","minimum":0}}},"required":["mutationIds"],"additionalProperties":false}}));
        }
        if self.skill_gated() {
            let skills = self.skills.lock().unwrap();
            list.retain(|tool| {
                let name = tool["name"].as_str().unwrap();
                crate::agent_workflows::CORE_TOOLS.contains(&name)
                    || skills.iter().any(|skill| {
                        crate::agent_workflows::skill_tools(skill)
                            .is_some_and(|tools| tools.iter().any(|t| t == name))
                    })
            });
        }
        list
    }

    fn work_call(&self, name: &str, args: &Value) -> Result<Value, String> {
        if name == "open_work" {
            let all = work::list(&self.out)?;
            let state = args["state"].as_str().unwrap_or("active");
            let rows: Vec<_> = all
                .iter()
                .filter(|r| match state {
                    "all" => true,
                    "expired" => r.state == "open" && !r.active(),
                    _ => r.active(),
                })
                .filter(|r| args["node"].as_str().is_none_or(|n| r.targets.contains(n)))
                .filter(|r| {
                    args["goal"]
                        .as_str()
                        .is_none_or(|g| r.goals.iter().any(|v| v == g))
                })
                .collect();
            let offset = args["cursor"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().unwrap_or(25).clamp(1, 100) as usize;
            return Ok(
                json!({"work":rows.iter().skip(offset).take(limit).map(|r|r.summary()).collect::<Vec<_>>(),"nextCursor":(offset+limit<rows.len()).then_some(offset+limit)}),
            );
        }
        if name == "unstage" {
            return self.unstage_work(args);
        }
        let id = args[if name == "begin_goals" {
            "workId"
        } else {
            "id"
        }]
        .as_str()
        .ok_or("work id is required")?;
        let mut r = work::read(&self.out, id)?;
        if name == "read_open_work" {
            return Ok(
                json!({"work":r.summary(),"mutations":r.mutations.iter().enumerate().map(|(i,m)|json!({"mutationId":r.mutation_ids.get(i),"mutation":m})).collect::<Vec<_>>(),"callGroups":r.call_groups,"callGroups":r.call_groups,"conflictNotices":self.work_notices(&r)}),
            );
        }
        if self.bridge.reader
            || !(self.write || self.modes.iter().any(|m| m == "compile" || m == "chat"))
        {
            return Err("read-only serving".into());
        }
        if self
            .bridge
            .only
            .as_ref()
            .is_some_and(|only| only != &r.batch && !r.goals.contains(only))
        {
            return Err("work is outside this serving's scope".into());
        }
        if name == "begin_goals" {
            if args["ownerToken"].as_str() != Some(&r.receipt.owner) {
                return Err(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/payloads/goals/prompts/inline/open-work/owner-token.md"
                ))
                .into());
            }
            if r.state == "committed" {
                return Ok(r.result);
            }
            if r.state != "open" {
                return Err("work was discarded".into());
            }
        } else {
            if args["expectedEpoch"].as_u64() != Some(r.receipt.epoch) {
                return Err("owner epoch changed".into());
            }
            let own = args["ownerToken"].as_str() == Some(&r.receipt.owner);
            if r.active() && (!own || name == "adopt_open_work") {
                return Err("active foreign work cannot be adopted or discarded".into());
            }
            if r.state != "open" {
                return Err("work is already closed".into());
            }
        }
        let sequence = r.sequence;
        r.identity["reason"] = args["reason"].clone();
        if name == "discard_open_work" {
            r.state = "discarded".into();
            r.payload = Value::Null;
            r.mutations.clear();
            r.targets.clear();
            work::save(&self.out, &mut r, Some(sequence))?;
            return Ok(json!({"discarded":id}));
        }
        if name == "adopt_open_work" {
            r.receipt.epoch += 1;
            r.identity["previousOwner"] = r.identity.clone();
            let mut token = [0u8; 24];
            getrandom::fill(&mut token).map_err(|e| e.to_string())?;
            r.receipt.owner = token.iter().map(|b| format!("{b:02x}")).collect::<String>();
            r.identity["workerId"] = json!(self.worker_id());
            r.identity["clientName"] = json!(self.client.lock().unwrap().clone());
        }
        r.last_activity = crate::sys::now_unix();
        r.expires_at = r.last_activity + crate::limits::OPEN_WORK_TTL_SECS;
        let mut batch: OpenBatch =
            serde_json::from_value(r.payload.clone()).map_err(|e| e.to_string())?;
        if batch.session.snapshot.out != self.out {
            return Err("open work belongs to another document set".into());
        }
        batch.work = Some(r.receipt.clone());
        batch.session.done = None;
        r.payload = serde_json::to_value(&batch).map_err(|e| e.to_string())?;
        work::save(&self.out, &mut r, Some(sequence))?;
        self.work_heartbeat(&mut batch);
        if let Some(old) = self.open.lock().unwrap().replace(batch) {
            self.parked.lock().unwrap().insert(old.id.clone(), old);
        }
        Ok(
            json!({"workId":r.receipt.id,"ownerToken":r.receipt.owner,"epoch":r.receipt.epoch,"batch":r.batch,"resumed":true,"package":self.open.lock().unwrap().as_ref().unwrap().session.render_status_full(),"conflictNotices":self.work_notices(&r)}),
        )
    }

    fn unstage_work(&self, args: &Value) -> Result<Value, String> {
        let mut guard = self.open.lock().unwrap();
        let o = guard.as_mut().ok_or("no open work")?;
        let before = serde_json::to_value(&*o).map_err(|e| e.to_string())?;
        let receipt = o.work.clone().ok_or("work is not persisted")?;
        let mut r = work::read(&self.out, &receipt.id)?;
        r.check(&receipt)?;
        let ids: BTreeSet<u64> = args["mutationIds"]
            .as_array()
            .ok_or("mutationIds required")?
            .iter()
            .map(|v| v.as_u64().ok_or("invalid mutation id"))
            .collect::<Result<_, _>>()?;
        if ids.iter().any(|id| !r.mutation_ids.contains(id)) {
            return Err("unknown mutation id".into());
        }
        if r.call_groups.iter().any(|group| {
            group.iter().any(|id| ids.contains(id)) && group.iter().any(|id| !ids.contains(id))
        }) {
            return Err(include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/open-work/unstage-atomic.md"
            ))
            .into());
        }
        let indices = r
            .mutation_ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| ids.contains(id).then_some(i))
            .collect();
        o.session.unstage(&indices).map_err(|e| e.message)?;
        r.mutations = r
            .mutations
            .iter()
            .zip(&r.mutation_ids)
            .filter(|(_, id)| !ids.contains(id))
            .map(|(m, _)| m.clone())
            .collect();
        r.mutation_ids.retain(|id| !ids.contains(id));
        r.call_groups
            .retain(|g| !g.iter().any(|id| ids.contains(id)));
        let sequence = r.sequence;
        r.payload = serde_json::to_value(&*o).map_err(|e| e.to_string())?;
        work::update_targets(&mut r);
        if let Err(e) = work::save(&self.out, &mut r, Some(sequence)) {
            *o = serde_json::from_value(before).map_err(|e| e.to_string())?;
            return Err(e);
        }
        Ok(json!({"unstaged":ids,"stillStaged":o.session.staged.len(),"claimsInvalidated":true}))
    }

    pub(super) fn work_notices(&self, r: &work::Record) -> Vec<Value> {
        let s = Store::load(&self.out);
        let mut notices = Vec::new();
        for g in r.snapshot_generation + 1..=s.status.generation {
            if let Some(e) =
                crate::goals::read_journal_entry(&crate::layout::journal_file(&self.out, g), g)
            {
                let mut targets: BTreeSet<_> = e.mutations.iter().flat_map(work::targets).collect();
                targets.extend(
                    e.field_changes
                        .iter()
                        .filter_map(|v| v["node"].as_str().map(String::from)),
                );
                if !targets.is_disjoint(&r.targets) && e.work_id.as_deref() != Some(&r.receipt.id) {
                    notices.push(json!({"generation":g,"workId":e.work_id,"targets":targets.intersection(&r.targets).collect::<Vec<_>>(),"overwrites":e.overwrites}));
                }
            }
        }
        notices
    }

    pub(super) fn restore_work_record(&self, batch: &mut OpenBatch) -> Result<bool, String> {
        let Some(receipt) = batch.work.clone() else {
            return Ok(false);
        };
        let _lock = work::Lock::acquire(&self.out)?;
        let mut record = work::read(&self.out, &receipt.id)?;
        if record.receipt.epoch != receipt.epoch || record.receipt.owner != receipt.owner {
            return Err("persisted work ownership changed since checkpoint".into());
        }
        if record.state == "discarded" {
            return Err("persisted work was explicitly discarded".into());
        }
        if record.state == "committed" {
            return Ok(true);
        }
        let mut latest: OpenBatch =
            serde_json::from_value(record.payload.clone()).map_err(|e| e.to_string())?;
        latest.work = Some(receipt);
        latest.session.done = None;
        if latest.session.snapshot.out != self.out || latest.id != batch.id {
            return Err("persisted work belongs to another scope".into());
        }
        let seq = record.sequence;
        record.expires_at = crate::sys::now_unix() + crate::limits::OPEN_WORK_TTL_SECS;
        work::save(&self.out, &mut record, Some(seq))?;
        *batch = latest;
        self.work_heartbeat(batch);
        Ok(false)
    }

    pub(crate) fn pause_open_work(&self) {
        if let Some(o) = self.open.lock().unwrap().as_mut() {
            o.heartbeat = None;
        }
        for o in self.parked.lock().unwrap().values_mut() {
            o.heartbeat = None;
        }
        for child in self.children.lock().unwrap().values() {
            child.pause_open_work();
        }
    }

    pub(super) fn work_heartbeat(&self, o: &mut OpenBatch) {
        let alive = std::sync::Arc::new(());
        let weak = std::sync::Arc::downgrade(&alive);
        o.heartbeat = Some(alive);
        let out = self.out.clone();
        let receipt = o.work.clone().unwrap();
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                gloo_timers::future::TimeoutFuture::new(
                    (crate::limits::OPEN_WORK_HEARTBEAT_SECS * 1000) as u32,
                )
                .await;
                if weak.upgrade().is_none() {
                    break;
                }
                let Ok(mut r) = work::read(&out, &receipt.id) else {
                    break;
                };
                if r.check(&receipt).is_err() {
                    break;
                }
                let seq = r.sequence;
                r.expires_at = crate::sys::now_unix() + crate::limits::OPEN_WORK_TTL_SECS;
                if work::save(&out, &mut r, Some(seq)).is_err() {
                    break;
                }
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        if crate::vfs::is_scoped() {
            let Ok(records) = crate::hosted::records::store() else {
                return;
            };
            let cancel = crate::hosted::open_work_cancel();
            let key = work::key(&out, &receipt.id);
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(
                    crate::limits::OPEN_WORK_HEARTBEAT_SECS,
                ));
                if weak.upgrade().is_none()
                    || cancel.as_ref().is_some_and(|(c, s)| {
                        c.load(std::sync::atomic::Ordering::Acquire)
                            || s.load(std::sync::atomic::Ordering::Acquire)
                    })
                {
                    break;
                }
                let Ok(Some(value)) = records.meta(crate::hosted::records::Stream::OpenWork, &key)
                else {
                    break;
                };
                let Ok(mut r) = serde_json::from_value::<work::Record>(value) else {
                    break;
                };
                if r.check(&receipt).is_err() {
                    break;
                }
                let seq = r.sequence;
                r.sequence += 1;
                r.heartbeat_at = crate::sys::now_unix();
                r.expires_at = crate::sys::now_unix() + crate::limits::OPEN_WORK_TTL_SECS;
                let Ok(value) = serde_json::to_value(&r) else {
                    break;
                };
                if records.compare_open_work(&key, Some(seq), &value).is_err() {
                    continue;
                }
            });
        } else {
            let cancel = self.bridge.cancel_file.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(
                    crate::limits::OPEN_WORK_HEARTBEAT_SECS,
                ));
                if weak.upgrade().is_none()
                    || cancel.as_ref().is_some_and(|p| crate::vfs::exists(p))
                {
                    break;
                }
                let Ok(_lock) = work::Lock::acquire(&out) else {
                    break;
                };
                let Ok(mut r) = work::read(&out, &receipt.id) else {
                    break;
                };
                if r.check(&receipt).is_err() {
                    break;
                }
                let seq = r.sequence;
                r.expires_at = crate::sys::now_unix() + crate::limits::OPEN_WORK_TTL_SECS;
                if work::save(&out, &mut r, Some(seq)).is_err() {
                    break;
                }
            });
        }
    }
}

fn host_name() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        return crate::sys::env_var("HOSTNAME").ok().or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });
    }
    None
}

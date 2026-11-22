mod capture;
mod db;
mod dialogue;
mod engine;
mod models;
mod planner;
mod tray;
mod vision;

use db::Database;
use models::*;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use tauri::{Manager, State};

#[derive(Clone)]
struct LiveSession {
    mode: String,
    context_id: String,
    context: Vec<TranslationResult>,
    transcript: Vec<TranslationResult>,
}
struct AppState {
    database: Mutex<Database>,
    client: reqwest::Client,
    gate: tokio::sync::Mutex<()>,
    context: Mutex<Vec<TranslationResult>>,
    transcript: Mutex<Vec<TranslationResult>>,
    batch_context: Mutex<Vec<TranslationResult>>,
    active_batch: Mutex<Option<String>>,
    live_sessions: Mutex<HashMap<String, LiveSession>>,
    revision: AtomicU64,
    data_directory: String,
}
const TRANSCRIPT_LIMIT: usize = 0; // Saved conversation history is retained until the user deletes it.
fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, Database>, String> {
    state
        .database
        .lock()
        .map_err(|_| "本地数据库暂时不可用，请重启软件。".into())
}
fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("com.moyu.translate", "deepseek-api-key")
        .map_err(|_| "无法访问系统凭据管理器。".into())
}
fn read_key() -> Result<Option<String>, String> {
    match key_entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("无法读取系统凭据，请检查 Windows 凭据管理器。".into()),
    }
}
fn invalidate(state: &AppState) {
    state.revision.fetch_add(1, Ordering::SeqCst);
}
fn settings_reset_context(previous: &Settings, next: &Settings) -> bool {
    previous.model != next.model
        || previous.use_memory != next.use_memory
        || previous.use_context != next.use_context
        || previous.save_history != next.save_history
}
fn context_matches(
    turn: &TranslationResult,
    request: &TranslationRequest,
    settings: &Settings,
) -> bool {
    turn.session_id == request.session_id
        && turn.source_language == request.source_language
        && turn.target_language == request.target_language
        && turn.style == request.style
        && turn.verified
        && (settings.use_memory || turn.origin != "memory")
}
fn clear_context(state: &AppState) -> Result<(), String> {
    state
        .context
        .lock()
        .map_err(|_| "上下文不可用，请重启软件。".to_string())?
        .clear();
    *state
        .active_batch
        .lock()
        .map_err(|_| "长文状态不可用。".to_string())? = None;
    state
        .batch_context
        .lock()
        .map_err(|_| "长文上下文不可用。".to_string())?
        .clear();
    Ok(())
}
fn reset_model_context(state: &AppState, db: &Database) -> Result<String, String> {
    let id = db.reset_context()?;
    clear_context(state)?;
    Ok(id)
}
fn reset_all_model_contexts(state: &AppState, db: &Database) -> Result<String, String> {
    let id = db.reset_all_contexts()?;
    clear_context(state)?;
    for live in state
        .live_sessions
        .lock()
        .map_err(|_| "会话缓存不可用。")?
        .values_mut()
    {
        live.context.clear();
        live.context_id.clear();
    }
    Ok(id)
}
fn transcript(state: &AppState) -> Result<Vec<TranslationResult>, String> {
    Ok(state
        .transcript
        .lock()
        .map_err(|_| "会话记录读取失败。".to_string())?
        .clone())
}
fn clear_transcript(state: &AppState) -> Result<(), String> {
    state
        .transcript
        .lock()
        .map_err(|_| "会话记录清理失败。".to_string())?
        .clear();
    Ok(())
}
fn append_bounded(turns: &mut Vec<TranslationResult>, result: &TranslationResult, limit: usize) {
    turns.push(result.clone());
    if turns.len() > limit {
        let extra = turns.len() - limit;
        turns.drain(..extra);
    }
}
fn record_result(
    state: &AppState,
    db: &Database,
    settings: &Settings,
    result: &TranslationResult,
) -> Result<(), String> {
    if settings.save_history {
        db.append_history(result)?;
    }
    {
        let mut turns = state
            .transcript
            .lock()
            .map_err(|_| "会话记录保存失败。".to_string())?;
        turns.push(result.clone());
    }
    if settings.use_context {
        let mut context = state
            .context
            .lock()
            .map_err(|_| "上下文保存失败。".to_string())?;
        append_bounded(&mut context, result, engine::MAX_CONTEXT_TURNS);
    }
    Ok(())
}

fn stash_session(state: &AppState, db: &Database) -> Result<(), String> {
    let session = db.session_id()?;
    let mode = db.workspace_mode()?;
    let live = LiveSession {
        mode: mode.clone(),
        context_id: db.context_id()?,
        context: state.context.lock().map_err(|_| "上下文不可用。")?.clone(),
        transcript: transcript(state)?,
    };
    let mut saved = state.live_sessions.lock().map_err(|_| "会话缓存不可用。")?;
    saved.retain(|id, value| id == &session || value.mode != mode);
    saved.insert(session, live);
    Ok(())
}
fn load_session(state: &AppState, db: &Database) -> Result<(), String> {
    let id = db.session_id()?;
    let context_id = db.context_id()?;
    let saved = state
        .live_sessions
        .lock()
        .map_err(|_| "会话缓存不可用。")?
        .get(&id)
        .cloned();
    clear_context(state)?;
    let context_enabled = db.settings()?.use_context;
    let (turns, context) = if let Some(live) = saved {
        (
            live.transcript,
            if context_enabled && live.context_id == context_id {
                live.context
            } else if context_enabled {
                db.recent_context(&id)?
            } else {
                Vec::new()
            },
        )
    } else {
        (
            db.session_turns(&id)?,
            if context_enabled {
                db.recent_context(&id)?
            } else {
                Vec::new()
            },
        )
    };
    *state.transcript.lock().map_err(|_| "会话读取失败。")? = turns;
    *state.context.lock().map_err(|_| "上下文读取失败。")? = context;
    Ok(())
}
fn dialogue_context(
    turns: &[TranslationResult],
    request: &TranslationRequest,
    settings: &Settings,
) -> Vec<TranslationResult> {
    let same_language = |a: &str, b: &str| a == b || (a.starts_with("zh-") && b.starts_with("zh-"));
    turns
        .iter()
        .filter_map(|turn| {
            if turn.session_id != request.session_id
                || turn.style != request.style
                || !turn.verified
                || (!settings.use_memory && turn.origin == "memory")
            {
                return None;
            }
            let mut oriented = turn.clone();
            if same_language(&turn.source_language, &request.source_language)
                && same_language(&turn.target_language, &request.target_language)
            {
            } else if same_language(&turn.source_language, &request.target_language)
                && same_language(&turn.target_language, &request.source_language)
            {
                std::mem::swap(&mut oriented.text, &mut oriented.translation);
            } else {
                return None;
            }
            oriented.source_language = request.source_language.clone();
            oriented.target_language = request.target_language.clone();
            Some(oriented)
        })
        .collect()
}

#[tauri::command]
fn get_bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let db = lock_db(&state)?;
    bootstrap(&state, &db)
}
fn bootstrap(state: &AppState, db: &Database) -> Result<Bootstrap, String> {
    let conversations = db.conversations("", 0, 40)?;
    Ok(Bootstrap {
        workspace_mode: db.workspace_mode()?,
        dialogue_preferences: db.dialogue_preferences(&db.session_id()?)?,
        settings: db.settings()?,
        has_api_key: read_key()?.is_some(),
        history: db.history()?,
        session_turns: transcript(state)?,
        conversations: conversations.conversations,
        has_more_conversations: conversations.has_more,
        memories: db.memories()?,
        terms: db.terms()?,
        data_directory: state.data_directory.clone(),
        session_id: db.session_id()?,
        context_id: db.context_id()?,
        context_limits: ContextLimits {
            max_turns: engine::MAX_CONTEXT_TURNS,
            max_characters: engine::MAX_CONTEXT_CHARS,
            transcript_limit: TRANSCRIPT_LIMIT,
        },
    })
}
#[tauri::command]
fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    engine::validate_settings(&settings)?;
    let db = lock_db(&state)?;
    let previous = db.settings()?;
    db.save_settings(&settings)?;
    if settings_reset_context(&previous, &settings) {
        reset_all_model_contexts(&state, &db)?;
    }
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn save_api_key(api_key: String, state: State<'_, AppState>) -> Result<(), String> {
    let key = api_key.trim();
    if !(16..=512).contains(&key.len())
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("API Key 格式不正确，请粘贴 DeepSeek 官方平台生成的密钥。".into());
    }
    let _db = lock_db(&state)?;
    key_entry()?
        .set_password(key)
        .map_err(|_| "无法安全保存 API Key，请检查系统凭据管理器。".to_string())?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn remove_api_key(state: State<'_, AppState>) -> Result<(), String> {
    let _db = lock_db(&state)?;
    match key_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(_) => return Err("无法删除系统中保存的 API Key。".into()),
    }
    invalidate(&state);
    Ok(())
}
#[tauri::command]
async fn translate(
    request: TranslationRequest,
    state: State<'_, AppState>,
) -> Result<TranslationResult, String> {
    engine::validate_request(&request)?;
    let _gate = state
        .gate
        .try_lock()
        .map_err(|_| "正在处理上一条翻译，请稍后再试。".to_string())?;
    let (settings, terms, context, memory, revision) = {
        let db = lock_db(&state)?;
        if request.session_id != db.session_id()? {
            return Err("上下文已更换，请重新翻译。".into());
        }
        let settings = db.settings()?;
        engine::validate_settings(&settings)?;
        let mut context = Vec::new();
        if settings.use_context {
            let live = state
                .context
                .lock()
                .map_err(|_| "上下文读取失败。".to_string())?;
            context = live
                .iter()
                .filter(|turn| turn.session_id == request.session_id)
                .cloned()
                .collect();
            if db.workspace_mode()? == "dialogue" {
                context = dialogue_context(&context, &request, &settings);
            } else {
                context.retain(|turn| context_matches(turn, &request, &settings));
            }
        }
        if let Some(batch_id) = &request.batch_id {
            let mut active = state
                .active_batch
                .lock()
                .map_err(|_| "长文状态不可用。".to_string())?;
            if active.as_ref() != Some(batch_id) {
                state
                    .batch_context
                    .lock()
                    .map_err(|_| "长文上下文不可用。".to_string())?
                    .clear();
                *active = Some(batch_id.clone());
            }
            if !settings.use_context {
                context = state
                    .batch_context
                    .lock()
                    .map_err(|_| "长文上下文不可用。".to_string())?
                    .iter()
                    .filter(|turn| context_matches(turn, &request, &settings))
                    .cloned()
                    .collect();
            }
        }
        let memory = if settings.use_memory && context.is_empty() {
            db.find_memory(&request, &settings)?
        } else {
            None
        };
        let terms = if settings.use_memory {
            db.matching_terms(&request)?
        } else {
            Vec::new()
        };
        (
            settings,
            terms,
            context,
            memory,
            state.revision.load(Ordering::SeqCst),
        )
    };
    let (translation, origin) = if request.text.trim().is_empty() {
        (request.text.clone(), "literal")
    } else if let Some(entry) = memory {
        (entry.translation, "memory")
    } else {
        let key = read_key()?.ok_or_else(|| "请先在设置中保存 DeepSeek API Key。".to_string())?;
        (
            engine::translate_checked(&state.client, &key, &settings, &request, &terms, &context)
                .await?,
            "model",
        )
    };
    let result = TranslationResult {
        id: uuid::Uuid::new_v4().to_string(),
        text: request.text,
        translation,
        source_language: request.source_language.clone(),
        target_language: request.target_language,
        detected_language: request.source_language,
        style: request.style,
        session_id: request.session_id,
        origin: origin.into(),
        verified: true,
        created_at: chrono::Utc::now().to_rfc3339(),
        source_image: request.source_image,
    };
    {
        let db = lock_db(&state)?;
        if revision != state.revision.load(Ordering::SeqCst)
            || result.session_id != db.session_id()?
        {
            return Err("设置或上下文已更换，本次结果已丢弃，请重新翻译。".into());
        }
        record_result(&state, &db, &settings, &result)?;
        if request.batch_id.is_some() {
            let mut batch_context = state
                .batch_context
                .lock()
                .map_err(|_| "长文上下文保存失败。".to_string())?;
            append_bounded(&mut batch_context, &result, engine::MAX_CONTEXT_TURNS);
        }
    }
    Ok(result)
}
#[tauri::command]
async fn confirm_memory(mut entry: MemoryInput, state: State<'_, AppState>) -> Result<(), String> {
    engine::validate_text(&entry.source_text, usize::MAX)?;
    engine::validate_text(&entry.translation, usize::MAX)?;
    engine::validate_language(&entry.source_language, true)?;
    engine::validate_language(&entry.target_language, false)?;
    engine::validate_style(&entry.style)?;
    entry.source_text = entry.source_text.trim().to_owned();
    entry.translation = entry.translation.trim().to_owned();
    let _gate = state
        .gate
        .try_lock()
        .map_err(|_| "正在检查译文，请稍后再试。".to_string())?;
    let (settings, request, terms, revision) = {
        let db = lock_db(&state)?;
        let request = TranslationRequest {
            text: entry.source_text.clone(),
            source_language: entry.source_language.clone(),
            target_language: entry.target_language.clone(),
            style: entry.style.clone(),
            session_id: db.session_id()?,
            batch_id: None,
            source_image: None,
        };
        engine::validate_request(&request)?;
        let settings = db.settings()?;
        engine::validate_settings(&settings)?;
        (
            settings,
            request.clone(),
            db.matching_terms(&request)?,
            state.revision.load(Ordering::SeqCst),
        )
    };
    let key =
        read_key()?.ok_or_else(|| "请先保存 DeepSeek API Key，以校验确认的译文。".to_string())?;
    engine::review_candidate(
        &state.client,
        &key,
        &settings,
        &request,
        &terms,
        &[],
        &entry.translation,
    )
    .await?;
    let db = lock_db(&state)?;
    if revision != state.revision.load(Ordering::SeqCst) {
        return Err("设置已变更，请重新确认译文。".into());
    }
    db.confirm_memory(&entry, &settings)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn delete_memory(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = lock_db(&state)?;
    db.delete_memory(&id)?;
    reset_all_model_contexts(&state, &db)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn clear_memories(state: State<'_, AppState>) -> Result<(), String> {
    let db = lock_db(&state)?;
    db.clear_memories()?;
    reset_all_model_contexts(&state, &db)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn upsert_term(mut term: TermInput, state: State<'_, AppState>) -> Result<(), String> {
    engine::validate_text(&term.source, 120)?;
    engine::validate_text(&term.target, 120)?;
    engine::validate_language(&term.source_language, true)?;
    engine::validate_language(&term.target_language, false)?;
    term.source = term.source.trim().to_owned();
    term.target = term.target.trim().to_owned();
    let db = lock_db(&state)?;
    db.upsert_term(&term)?;
    reset_all_model_contexts(&state, &db)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn delete_term(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = lock_db(&state)?;
    db.delete_term(&id)?;
    reset_all_model_contexts(&state, &db)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = lock_db(&state)?;
    db.clear_history()?;
    state
        .live_sessions
        .lock()
        .map_err(|_| "会话缓存不可用。")?
        .clear();
    db.new_session()?;
    clear_context(&state)?;
    clear_transcript(&state)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn delete_history(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = lock_db(&state)?;
    db.delete_history(&id)?;
    for live in state
        .live_sessions
        .lock()
        .map_err(|_| "会话缓存不可用。")?
        .values_mut()
    {
        live.transcript.retain(|turn| turn.id != id);
        live.context.retain(|turn| turn.id != id);
    }
    state
        .transcript
        .lock()
        .map_err(|_| "会话记录清理失败。".to_string())?
        .retain(|turn| turn.id != id);
    reset_model_context(&state, &db)?;
    invalidate(&state);
    Ok(())
}
#[tauri::command]
fn new_session(state: State<'_, AppState>) -> Result<String, String> {
    let db = lock_db(&state)?;
    let session = db.new_session()?;
    clear_context(&state)?;
    clear_transcript(&state)?;
    invalidate(&state);
    Ok(session)
}
#[tauri::command]
fn get_session_turns(state: State<'_, AppState>) -> Result<Vec<TranslationResult>, String> {
    let _db = lock_db(&state)?;
    transcript(&state)
}
#[tauri::command]
fn reset_context(state: State<'_, AppState>) -> Result<String, String> {
    let db = lock_db(&state)?;
    let id = reset_model_context(&state, &db)?;
    invalidate(&state);
    Ok(id)
}

#[tauri::command]
fn plan_translation(request: TranslationRequest) -> Result<TranslationPlan, String> {
    engine::validate_request(&request)?;
    Ok(planner::plan(&request.text))
}

#[tauri::command]
async fn extract_image(
    request: ImageInput,
    state: State<'_, AppState>,
) -> Result<ImageExtraction, String> {
    let _gate = state
        .gate
        .try_lock()
        .map_err(|_| "正在处理上一条翻译，请稍后再试。".to_string())?;
    let revision = state.revision.load(Ordering::SeqCst);
    let prepared = tauri::async_runtime::spawn_blocking(move || vision::prepare(request))
        .await
        .map_err(|_| "图片处理失败，请重试。".to_string())??;
    let key = read_key()?
        .ok_or_else(|| "请先在设置中保存 DeepSeek API Key，以识别图片文字。".to_string())?;
    let result = vision::extract(&state.client, &key, prepared).await?;
    if revision != state.revision.load(Ordering::SeqCst) {
        return Err("会话或设置已变更，图片识别结果已丢弃，请重试。".into());
    }
    Ok(result)
}

#[tauri::command]
fn list_conversations(
    query: String,
    offset: usize,
    state: State<'_, AppState>,
) -> Result<ConversationPage, String> {
    lock_db(&state)?.conversations(query.trim(), offset, 40)
}
#[tauri::command]
fn open_conversation(id: String, state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let db = lock_db(&state)?;
    stash_session(&state, &db)?;
    db.open_session(&id)?;
    load_session(&state, &db)?;
    invalidate(&state);
    bootstrap(&state, &db)
}
#[tauri::command]
fn delete_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = lock_db(&state)?;
    let current = db.session_id()? == id;
    let mode = db.workspace_mode()?;
    db.delete_session(&id)?;
    state
        .live_sessions
        .lock()
        .map_err(|_| "会话缓存不可用。")?
        .remove(&id);
    if current {
        db.new_session_in_mode(&mode)?;
        clear_context(&state)?;
        clear_transcript(&state)?;
    }
    invalidate(&state);
    Ok(())
}

#[tauri::command]
fn switch_workspace(mode: String, state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let _gate = state
        .gate
        .try_lock()
        .map_err(|_| "当前翻译尚未完成，请稍后切换模式。")?;
    let db = lock_db(&state)?;
    stash_session(&state, &db)?;
    db.switch_workspace(&mode)?;
    load_session(&state, &db)?;
    invalidate(&state);
    bootstrap(&state, &db)
}
#[tauri::command]
fn save_dialogue_preferences(
    peer_language: String,
    state: State<'_, AppState>,
) -> Result<Bootstrap, String> {
    dialogue::validate_peer_language(&peer_language)?;
    let _gate = state
        .gate
        .try_lock()
        .map_err(|_| "请等待当前翻译完成后修改客户语言。")?;
    let db = lock_db(&state)?;
    if db.workspace_mode()? != "dialogue" {
        return Err("请先打开双向对话模式。".into());
    }
    let session = db.session_id()?;
    let mut preferences = db.dialogue_preferences(&session)?;
    preferences.peer_language = peer_language;
    db.save_dialogue_preferences(&session, &preferences)?;
    bootstrap(&state, &db)
}
#[tauri::command]
async fn resolve_dialogue(
    request: DialogueInput,
    state: State<'_, AppState>,
) -> Result<DialogueRoute, String> {
    dialogue::validate_input(&request)?;
    let _gate = state
        .gate
        .try_lock()
        .map_err(|_| "正在处理上一条翻译，请稍后再试。")?;
    let (preferences, revision) = {
        let db = lock_db(&state)?;
        if db.session_id()? != request.session_id || db.workspace_mode()? != "dialogue" {
            return Err("双向会话已更换，请重新提交。".into());
        }
        (
            db.dialogue_preferences(&request.session_id)?,
            state.revision.load(Ordering::SeqCst),
        )
    };
    if preferences.peer_language != request.peer_language {
        return Err("客户语言已更新，请重新提交。".into());
    }
    let configured_key = read_key()?;
    if request.direction == "auto"
        && request.text.chars().any(char::is_alphabetic)
        && configured_key.is_none()
    {
        return Err("请先在设置中保存 DeepSeek API Key，以识别对话语言。".into());
    }
    let key = configured_key.unwrap_or_default();
    let route = dialogue::resolve(
        &state.client,
        &key,
        &request,
        &preferences.last_peer_language,
        &preferences.last_direction,
    )
    .await?;
    let db = lock_db(&state)?;
    if revision != state.revision.load(Ordering::SeqCst)
        || db.session_id()? != request.session_id
        || db.dialogue_preferences(&request.session_id)?.peer_language != request.peer_language
    {
        return Err("会话或设置已更换，已丢弃旧的语言识别结果。".into());
    }
    let updated = DialoguePreferences {
        peer_language: preferences.peer_language,
        last_peer_language: route.peer_language.clone(),
        last_direction: route.direction.clone(),
    };
    db.save_dialogue_preferences(&request.session_id, &updated)?;
    Ok(route)
}

#[tauri::command]
fn hide_to_tray(app: tauri::AppHandle) -> Result<(), String> {
    if app.tray_by_id("moyu-tray").is_none() {
        return Err("系统托盘未就绪，请保留当前窗口。".into());
    }
    app.get_webview_window("main")
        .ok_or_else(|| "主窗口不可用。".to_string())?
        .hide()
        .map_err(|_| "无法收起到系统托盘。".to_string())
}

#[tauri::command]
fn get_capture_preferences(app: tauri::AppHandle) -> Result<CaptureStatus, String> {
    capture::status(&app)
}

#[tauri::command]
async fn set_capture_shortcut(
    shortcut: String,
    app: tauri::AppHandle,
) -> Result<CaptureStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = lock_db(&state)?;
        let previous = db.capture_shortcut()?;
        let next = capture::configure(&app, &shortcut)?;
        if let Err(error) = db.save_capture_shortcut(&next.shortcut) {
            let _ = capture::configure(&app, &previous);
            return Err(error);
        }
        Ok(next)
    })
    .await
    .map_err(|_| "无法保存截屏快捷键，请重试。".to_string())?
}

#[tauri::command]
fn start_capture(app: tauri::AppHandle) -> Result<(), String> {
    capture::start_capture(&app)
}

#[tauri::command]
async fn set_capture_recording(app: tauri::AppHandle, recording: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || capture::set_recording(&app, recording))
        .await
        .map_err(|_| "无法切换快捷键录入状态，请重试。".to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let database =
                Database::open(&directory.join("moyu.sqlite3")).map_err(std::io::Error::other)?;
            let capture_shortcut = database.capture_shortcut().map_err(std::io::Error::other)?;
            let current_session = database.session_id().map_err(std::io::Error::other)?;
            let session_turns = database
                .session_turns(&current_session)
                .map_err(std::io::Error::other)?;
            let context = if database
                .settings()
                .map_err(std::io::Error::other)?
                .save_history
            {
                database
                    .recent_context(&current_session)
                    .map_err(std::io::Error::other)?
            } else {
                Vec::new()
            };
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(180))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(format!("Moyu-Translate/{}", env!("CARGO_PKG_VERSION")))
                .build()?;
            app.manage(AppState {
                database: Mutex::new(database),
                client,
                gate: tokio::sync::Mutex::new(()),
                context: Mutex::new(context),
                transcript: Mutex::new(session_turns),
                batch_context: Mutex::new(Vec::new()),
                active_batch: Mutex::new(None),
                live_sessions: Mutex::new(HashMap::new()),
                revision: AtomicU64::new(0),
                data_directory: directory.to_string_lossy().into_owned(),
            });
            tray::install(app)?;
            // Capture failure must leave translation and tray access available.
            if capture::initialize(app.handle()).is_ok() {
                let _ = capture::configure(app.handle(), &capture_shortcut);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = capture::set_recording(&app, false);
                });
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().tray_by_id("moyu-tray").is_some() && window.hide().is_ok() {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            switch_workspace,
            resolve_dialogue,
            save_dialogue_preferences,
            save_settings,
            save_api_key,
            remove_api_key,
            translate,
            confirm_memory,
            delete_memory,
            clear_memories,
            upsert_term,
            delete_term,
            clear_history,
            delete_history,
            new_session,
            get_session_turns,
            reset_context,
            plan_translation,
            hide_to_tray,
            extract_image,
            list_conversations,
            open_conversation,
            delete_conversation,
            get_capture_preferences,
            set_capture_shortcut,
            start_capture,
            set_capture_recording
        ])
        .run(tauri::generate_context!())
        .expect("无法启动墨译，请检查 WebView2 和本地数据目录权限。");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Uses configured DeepSeek credentials and synthetic bilingual customer messages"]
    fn configured_account_bilingual_customer_translation_smoke() {
        let key = read_key().unwrap().expect("No configured DeepSeek key");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(180)).redirect(reqwest::redirect::Policy::none()).build().unwrap();
            let settings = Settings::default();
            let session = uuid::Uuid::new_v4().to_string();
            let mut peer = "en".to_string();
            let mut last = "inbound".to_string();
            let mut turns = Vec::new();
            for (text, expected_source, expected_target, identifier) in [
                ("Could you confirm whether item AX-204 is in stock?", "en", "zh-CN", "AX-204"),
                ("AX-204 目前有现货，明天发货。", "zh-CN", "en", "AX-204"),
                ("Ist die Bestellung 4821 bereits versandt?", "de", "zh-CN", "4821"),
                ("订单 4821 已发货，追踪号为 DE778899。", "zh-CN", "de", "DE778899"),
            ] {
                let route = dialogue::resolve(&client,&key,&DialogueInput { text:text.into(),session_id:session.clone(),peer_language:"auto".into(),direction:"auto".into() },&peer,&last).await.expect("Language routing failed");
                assert_eq!(route.source_language,expected_source);
                assert_eq!(route.target_language,expected_target);
                let request = TranslationRequest { text:text.into(),source_language:route.source_language.clone(),target_language:route.target_language.clone(),style:"natural".into(),session_id:session.clone(),batch_id:None,source_image:None };
                let context = dialogue_context(&turns,&request,&settings);
                let translated = engine::translate_checked(&client,&key,&settings,&request,&[],&context).await.expect("Translation or semantic verification failed");
                assert!(translated.contains(identifier), "Synthetic product/order identifier changed");
                if expected_target == "zh-CN" { assert!(translated.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))); }
                else { assert!(!translated.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))); }
                println!("Synthetic {} -> {} | {} | {}",route.source_language,route.target_language,text,translated);
                let mut result = dialogue_turn(&session,&route.source_language,&route.target_language);
                result.text = text.into(); result.translation = translated; turns.push(result);
                peer=route.peer_language;last=route.direction;
            }
            println!("Synthetic bilingual routing, aligned context, faithful translation and semantic verification passed.");
        });
    }

    fn dialogue_turn(session: &str, source: &str, target: &str) -> TranslationResult {
        TranslationResult {
            id: "context".into(),
            text: "Could you confirm the size?".into(),
            translation: "请确认尺寸。".into(),
            source_language: source.into(),
            target_language: target.into(),
            style: "natural".into(),
            session_id: session.into(),
            detected_language: source.into(),
            origin: "model".into(),
            verified: true,
            created_at: String::new(),
            source_image: None,
        }
    }
    #[test]
    fn dialogue_context_keeps_both_directions_aligned_and_never_mixes_languages() {
        let request = TranslationRequest {
            text: "是大号。".into(),
            source_language: "zh-CN".into(),
            target_language: "en".into(),
            style: "natural".into(),
            session_id: "customer-a".into(),
            batch_id: None,
            source_image: None,
        };
        let inbound = dialogue_turn("customer-a", "en", "zh-CN");
        let same = dialogue_turn("customer-a", "zh-TW", "en");
        let mut unchecked = inbound.clone();
        unchecked.verified = false;
        let mut memory = inbound.clone();
        memory.origin = "memory".into();
        let turns = vec![
            inbound.clone(),
            same,
            dialogue_turn("other-customer", "en", "zh-CN"),
            dialogue_turn("customer-a", "de", "zh-CN"),
            unchecked,
            memory,
        ];
        let settings = Settings {
            use_memory: false,
            ..Settings::default()
        };
        let oriented = dialogue_context(&turns, &request, &settings);
        assert_eq!(oriented.len(), 2);
        assert_eq!(oriented[0].text, inbound.translation);
        assert_eq!(oriented[0].translation, inbound.text);
        assert!(oriented
            .iter()
            .all(|turn| turn.source_language == "zh-CN" && turn.target_language == "en"));
        assert!(!context_matches(&inbound, &request, &settings));
    }
    #[test]
    fn switching_workspaces_retains_unsaved_transcripts_but_resets_do_not_resurrect_context() {
        let db = Database::open(std::path::Path::new(":memory:")).unwrap();
        let settings = Settings {
            save_history: false,
            ..Settings::default()
        };
        db.save_settings(&settings).unwrap();
        let state = AppState {
            database: Mutex::new(db),
            client: reqwest::Client::new(),
            gate: tokio::sync::Mutex::new(()),
            context: Mutex::new(Vec::new()),
            transcript: Mutex::new(Vec::new()),
            batch_context: Mutex::new(Vec::new()),
            active_batch: Mutex::new(None),
            live_sessions: Mutex::new(HashMap::new()),
            revision: AtomicU64::new(0),
            data_directory: String::new(),
        };
        let db = lock_db(&state).unwrap();
        let standard = db.session_id().unwrap();
        let turn = dialogue_turn(&standard, "en", "zh-CN");
        record_result(&state, &db, &settings, &turn).unwrap();
        stash_session(&state, &db).unwrap();
        let dialogue = db.switch_workspace("dialogue").unwrap();
        load_session(&state, &db).unwrap();
        assert!(transcript(&state).unwrap().is_empty());
        record_result(
            &state,
            &db,
            &settings,
            &dialogue_turn(&dialogue, "en", "zh-CN"),
        )
        .unwrap();
        stash_session(&state, &db).unwrap();
        db.switch_workspace("standard").unwrap();
        load_session(&state, &db).unwrap();
        assert_eq!(transcript(&state).unwrap(), vec![turn]);
        reset_all_model_contexts(&state, &db).unwrap();
        stash_session(&state, &db).unwrap();
        db.switch_workspace("dialogue").unwrap();
        load_session(&state, &db).unwrap();
        assert_eq!(transcript(&state).unwrap().len(), 1);
        assert!(state.context.lock().unwrap().is_empty());
        assert!(db.history().unwrap().is_empty());
    }

    #[test]
    fn independent_nonpersistent_translation_retains_complete_readable_transcript() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let session_id = database.session_id().unwrap();
        let settings = Settings {
            use_context: false,
            save_history: false,
            ..Settings::default()
        };
        let state = AppState {
            database: Mutex::new(database),
            client: reqwest::Client::new(),
            gate: tokio::sync::Mutex::new(()),
            context: Mutex::new(Vec::new()),
            transcript: Mutex::new(Vec::new()),
            batch_context: Mutex::new(Vec::new()),
            active_batch: Mutex::new(None),
            live_sessions: Mutex::new(HashMap::new()),
            revision: AtomicU64::new(0),
            data_directory: String::new(),
        };
        let db = lock_db(&state).unwrap();
        for index in 0..1_005 {
            let result = TranslationResult {
                id: index.to_string(),
                text: format!("source-{index}"),
                translation: "译文".into(),
                source_language: "en".into(),
                target_language: "zh-CN".into(),
                style: "natural".into(),
                session_id: session_id.clone(),
                detected_language: "en".into(),
                origin: "model".into(),
                verified: true,
                created_at: String::new(),
                source_image: None,
            };
            record_result(&state, &db, &settings, &result).unwrap();
        }
        let visible = transcript(&state).unwrap();
        assert_eq!(visible.len(), 1_005);
        assert_eq!(visible[0].id, "0");
        assert_eq!(visible.last().unwrap().id, "1004");
        assert!(state.context.lock().unwrap().is_empty());
        assert!(db.history().unwrap().is_empty());
    }
    #[test]
    fn resetting_model_context_preserves_visible_transcript_and_material() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let session_id = database.session_id().unwrap();
        let turn = TranslationResult {
            id: "turn".into(),
            text: "Previous sentence".into(),
            translation: "上一句".into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
            style: "natural".into(),
            session_id: session_id.clone(),
            detected_language: "en".into(),
            origin: "model".into(),
            verified: true,
            created_at: String::new(),
            source_image: None,
        };
        let state = AppState {
            database: Mutex::new(database),
            client: reqwest::Client::new(),
            gate: tokio::sync::Mutex::new(()),
            context: Mutex::new(vec![turn.clone()]),
            transcript: Mutex::new(vec![turn.clone()]),
            batch_context: Mutex::new(Vec::new()),
            active_batch: Mutex::new(None),
            live_sessions: Mutex::new(HashMap::new()),
            revision: AtomicU64::new(0),
            data_directory: String::new(),
        };
        let db = lock_db(&state).unwrap();
        let previous_context = db.context_id().unwrap();
        assert_ne!(reset_model_context(&state, &db).unwrap(), previous_context);
        assert!(state.context.lock().unwrap().is_empty());
        assert_eq!(transcript(&state).unwrap(), vec![turn]);
        assert_eq!(db.session_id().unwrap(), session_id);
    }
    #[test]
    fn disabling_memory_or_persistence_starts_new_context() {
        let previous = Settings::default();
        let mut next = previous.clone();
        next.use_memory = false;
        assert!(settings_reset_context(&previous, &next));
        next = previous.clone();
        next.save_history = false;
        assert!(settings_reset_context(&previous, &next));
        next = previous.clone();
        next.use_context = false;
        assert!(settings_reset_context(&previous, &next));
        next = previous.clone();
        next.model = "deepseek-v4-pro".into();
        assert!(settings_reset_context(&previous, &next));
        assert!(!settings_reset_context(&previous, &previous));
    }
    #[test]
    fn context_rejects_other_sessions_directions_unchecked_or_disabled_memory() {
        let request = TranslationRequest {
            text: "Next sentence".into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
            style: "natural".into(),
            session_id: "same-session".into(),
            batch_id: None,
            source_image: None,
        };
        let mut turn = TranslationResult {
            id: "turn".into(),
            text: "Previous sentence".into(),
            translation: "上一句".into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
            style: "natural".into(),
            session_id: "same-session".into(),
            detected_language: "en".into(),
            origin: "model".into(),
            verified: true,
            created_at: String::new(),
            source_image: None,
        };
        let mut settings = Settings::default();
        assert!(context_matches(&turn, &request, &settings));
        turn.session_id = "other-session".into();
        assert!(!context_matches(&turn, &request, &settings));
        turn.session_id = request.session_id.clone();
        turn.target_language = "ja".into();
        assert!(!context_matches(&turn, &request, &settings));
        turn.target_language = request.target_language.clone();
        turn.verified = false;
        assert!(!context_matches(&turn, &request, &settings));
        turn.verified = true;
        turn.origin = "memory".into();
        settings.use_memory = false;
        assert!(!context_matches(&turn, &request, &settings));
    }
}

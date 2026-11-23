// A separate Vite test entry. Never imported by production source or index.html.
import React, { useCallback, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import type { Bootstrap, DialoguePreferences, DialogueRoute, DialogueInput, ImageExtraction, MemoryInput, Settings, TranslationPlan, TranslationRequest, TranslationResult } from '../../src/types';
import '../../src/styles.css';

type FixtureMode = 'standard' | 'dialogue';
type FixtureDialoguePreferences = DialoguePreferences;
type FixtureBootstrap = Bootstrap & { workspaceMode: FixtureMode; dialoguePreferences: FixtureDialoguePreferences };
type FixtureDialogueRoute = DialogueRoute;
const defaultDialogue = (): FixtureDialoguePreferences => ({ peerLanguage: 'auto', lastPeerLanguage: 'en', lastDirection: 'inbound' });

type Deferred<T> = { resolve: (value: T) => void; reject: (reason: string) => void };
let material = 1;
let serial = 1;
const uuid = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`;
const clone = <T,>(value: T): T => structuredClone(value);
const settings: Settings = { model: 'deepseek-flash', style: 'natural', useMemory: true, useContext: true, saveHistory: true };
const sourceParagraph = 'A useful translation preserves terminology and continuity across paragraphs. The local transcript remains available for reading, while only a bounded recent window is sent to the translation engine. ';
const targetParagraph = '好的翻译会在段落之间保留术语与称谓的一致性。本地记录方便继续回看，模型每次只参考受限制的最近片段。对于长文章，原文与译文应当清楚地对应，阅读过程也应保持从容。';
function fixtureData(size: number, mode: FixtureMode = 'standard'): FixtureBootstrap {
  const sessionId = uuid(material);
  const rows: TranslationResult[] = Array.from({ length: size }, (_, index) => ({
    id: `fixture-${material}-${index + 1}`, sessionId, text: `SOURCE ${index + 1}: ${index % 7 === 0 || index === size - 1 ? sourceParagraph.repeat(7) : sourceParagraph}`,
    translation: `第 ${index + 1} 段译文：${index % 9 === 0 || index === size - 1 ? `${targetParagraph}\n\n`.repeat(7) : targetParagraph}`,
    sourceLanguage: 'en', targetLanguage: 'zh-CN', style: 'natural', detectedLanguage: 'en',
    verified: true, origin: 'model', createdAt: new Date(Date.UTC(2026, 9, 18, 8, 0, index)).toISOString(),
  }));
  return { workspaceMode: mode, dialoguePreferences: defaultDialogue(), settings: clone(settings), hasApiKey: true, sessionTurns: rows, history: rows.slice(-200).reverse(), memories: [], terms: [],
    conversations: [], hasMoreConversations: false,
    dataDirectory: '本地测试夹具 · 无真实数据库', sessionId, contextId: uuid(10000 + material),
    contextLimits: { maxTurns: 12, maxCharacters: 24000, transcriptLimit: 0 } };
}
let snapshot = fixtureData(85);
let translatePending: (Deferred<TranslationResult> & { request: TranslationRequest }) | null = null;
let rememberPending: (Deferred<void> & { entry: MemoryInput }) | null = null;
let sessionPending: Deferred<string> | null = null;
let extractPending: (Deferred<ImageExtraction> & { request: { dataUrl: string; name: string } }) | null = null;
let publish: (() => Promise<Bootstrap>) | null = null;
const calls: Array<{ command: string; args: unknown }> = [];
let maximumConcurrentTranslations = 0;
let maximumConcurrentRequests = 0;
let capturePreferences = { shortcut: 'Ctrl+Alt+T', active: true, error: '' };
let rejectShortcutSave = false;
let captureStarts = 0;
let captureRecording = false;
let nextDialogueRoute: Partial<FixtureDialogueRoute> | null = null;
let rejectNextDialogueRoute = false;
let deferNextDialogueRoute = false;
let dialoguePending: (Deferred<FixtureDialogueRoute> & { route: FixtureDialogueRoute; request: DialogueInput }) | null = null;
const workspaceSnapshots = new Map<FixtureMode, FixtureBootstrap>();
const archivedConversations = new Map<string, { title: string; rows: TranslationResult[]; contextId: string; createdAt: string; updatedAt: string; mode?: FixtureMode; preferences?: FixtureDialoguePreferences }>();
const isHistoryFixture = new URLSearchParams(window.location.search).has('history');
function saveCurrentFixtureConversation() {
  if (!snapshot.sessionTurns.length) return;
  const old = archivedConversations.get(snapshot.sessionId);
  archivedConversations.set(snapshot.sessionId, { title: old?.title ?? snapshot.sessionTurns[0].text.slice(0, 60), contextId: snapshot.contextId,
    mode: snapshot.workspaceMode, preferences: clone(snapshot.dialoguePreferences), rows: clone(snapshot.sessionTurns), createdAt: old?.createdAt ?? snapshot.sessionTurns[0].createdAt,
    updatedAt: snapshot.sessionTurns.at(-1)!.createdAt });
}
function conversationSummaries(query = '', offset = 0) {
  const matching = [...archivedConversations].map(([id, value]) => ({ id, title: value.title, mode: value.mode ?? 'standard',
    preview: value.rows.at(-1)?.translation ?? '', turnCount: value.rows.length,
    createdAt: value.createdAt, updatedAt: value.updatedAt }))
    .filter((value) => !query || `${value.title}\n${value.preview}`.toLowerCase().includes(query.toLowerCase()))
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  return { conversations: matching.slice(offset, offset + 40), hasMore: matching.length > offset + 40 };
}
function bootstrapSnapshot() {
  const page = conversationSummaries();
  snapshot.conversations = page.conversations;
  workspaceSnapshots.set(snapshot.workspaceMode, clone(snapshot));
  snapshot.hasMoreConversations = page.hasMore;
  return clone(snapshot);
}
function seedConversationHistory() {
  archivedConversations.clear();
  for (let index = 0; index < 85; index += 1) {
    material = index + 100;
    const data = fixtureData(index === 84 ? 1250 : 3);
    const date = new Date(Date.UTC(2026, 9, 20, 8, 0, -index)).toISOString();
    const title = index === 84 ? '归档长文 · 1,250 段 · 超过旧 200 条' : `历史材料 ${String(index + 1).padStart(2, '0')}`;
    data.sessionTurns.forEach((turn) => { turn.createdAt = date; });
    archivedConversations.set(data.sessionId, { title, rows: data.sessionTurns, contextId: data.contextId, createdAt: date, updatedAt: date });
  }
  material = 200;
  snapshot = fixtureData(0);
  bootstrapSnapshot();
}

// Deterministic UI-only chunking, deliberately independent from the production
// Rust token planner. UI tests control each completion and never call a model.
function fixturePlan(request: TranslationRequest): TranslationPlan {
  const characters = Array.from(request.text);
  const segments = [];
  for (let index = 0; index < characters.length; index += 30000) {
    segments.push(characters.slice(index, index + 30000).join(''));
  }
  return { segments, estimatedInputTokens: characters.length, outputTokenBudget: 32768 };
}

// Deliberately deterministic mock routes. This is NOT production language detection;
// tests cover routing/state transport here, while real detection is verified separately.
function fixtureDialogueRoute(request: Pick<DialogueInput, 'text' | 'peerLanguage' | 'direction'>): FixtureDialogueRoute {
  const preference = snapshot.dialoguePreferences;
  const fixed = request.peerLanguage !== 'auto';
  const peer = fixed ? request.peerLanguage : preference.lastPeerLanguage;
  const text = request.text.trim();
  const source = /^(12345|SKU-A12|OK|👍)$/.test(text) ? 'und'
    : /^(Hallo|Guten)/.test(text) ? 'de'
      : /^(Bonjour|Je voudrais)/.test(text) ? 'fr'
        : /^(Necesito|Hola)/.test(text) ? 'es'
          : /[\u4e00-\u9fff]/.test(text) ? 'zh-CN' : 'en';
  const direction = request.direction === 'outbound' || request.direction === 'inbound'
    ? request.direction : source === 'und' ? preference.lastDirection : source.startsWith('zh') ? 'outbound' : 'inbound';
  const route: FixtureDialogueRoute = direction === 'outbound'
    ? { sourceLanguage: 'zh-CN', targetLanguage: peer, peerLanguage: peer, direction, usedFallback: source === 'und' && request.direction === 'auto' }
    : { sourceLanguage: request.direction === 'inbound' || source === 'und' ? peer : source, targetLanguage: 'zh-CN', peerLanguage: fixed || source === 'und' || source.startsWith('zh') ? peer : source, direction, usedFallback: source === 'und' && request.direction === 'auto' };
  return { ...route, ...nextDialogueRoute };
}
function acceptFixtureRoute(route: FixtureDialogueRoute) {
  snapshot.dialoguePreferences.lastPeerLanguage = route.peerLanguage;
  snapshot.dialoguePreferences.lastDirection = route.direction;
  return clone(route);
}

// core.isTauri() in the installed official package reads this flag.
Object.assign(globalThis, { isTauri: true });
mockIPC((command, args) => {
  calls.push({ command, args: clone(args ?? {}) });
  switch (command) {
    case 'get_bootstrap': return bootstrapSnapshot();
    case 'switch_workspace': {
      saveCurrentFixtureConversation();
      workspaceSnapshots.set(snapshot.workspaceMode, clone(snapshot));
      const mode = (args as { mode: FixtureMode }).mode;
      const oldSettings = clone(snapshot.settings);
      const saved = workspaceSnapshots.get(mode);
      if (saved) snapshot = clone(saved);
      else { material += 1; snapshot = fixtureData(0, mode); }
      snapshot.settings = oldSettings;
      return bootstrapSnapshot();
    }
    case 'save_dialogue_preferences': {
      const request = args as { peerLanguage: DialoguePreferences['peerLanguage'] };
      snapshot.dialoguePreferences.peerLanguage = request.peerLanguage;
      if (request.peerLanguage !== 'auto') snapshot.dialoguePreferences.lastPeerLanguage = request.peerLanguage;
      saveCurrentFixtureConversation();
      return bootstrapSnapshot();
    }
    case 'resolve_dialogue': {
      const request = (args as { request: DialogueInput }).request;
      if (translatePending || extractPending || dialoguePending) { maximumConcurrentRequests = 2; throw new Error('测试夹具：语言识别不能与其他模型请求并发'); }
      maximumConcurrentRequests = Math.max(maximumConcurrentRequests, 1);
      if (rejectNextDialogueRoute) { rejectNextDialogueRoute = false; throw new Error('测试语言识别失败：原文已保留，请重试。'); }
      const route = fixtureDialogueRoute(request); nextDialogueRoute = null;
      if (deferNextDialogueRoute) {
        deferNextDialogueRoute = false;
        return new Promise<FixtureDialogueRoute>((resolve, reject) => { dialoguePending = { resolve, reject, route, request: clone(request) }; });
      }
      return acceptFixtureRoute(route);
    }

    case 'get_session_turns': return clone(snapshot.sessionTurns);
    case 'get_capture_preferences': return clone(capturePreferences);
    case 'set_capture_shortcut': {
      if (rejectShortcutSave) { rejectShortcutSave = false; throw new Error('此快捷键已被其他应用占用，请更换组合。'); }
      const shortcut = (args as { shortcut: string }).shortcut;
      capturePreferences = { shortcut, active: !!shortcut, error: '' };
      return clone(capturePreferences);
    }
    case 'start_capture': captureStarts += 1; return;
    case 'set_capture_recording': captureRecording = (args as { recording: boolean }).recording; return;
    case 'list_conversations': {
      const request = args as { query?: string; offset?: number };
      saveCurrentFixtureConversation();
      return clone(conversationSummaries(request.query, request.offset));
    }
    case 'open_conversation': {
      saveCurrentFixtureConversation();
      const id = (args as { id: string }).id;
      const archived = archivedConversations.get(id);
      if (!archived) throw new Error('这份本地测试材料已不存在');
      snapshot.sessionId = id; snapshot.contextId = archived.contextId;
      snapshot.workspaceMode = archived.mode ?? 'standard'; snapshot.dialoguePreferences = clone(archived.preferences ?? defaultDialogue());
      snapshot.sessionTurns = clone(archived.rows);
      snapshot.history = [...snapshot.sessionTurns].reverse().slice(0, 200);
      return bootstrapSnapshot();
    }
    case 'delete_conversation': {
      const id = (args as { id: string }).id;
      archivedConversations.delete(id);
      if (snapshot.sessionId === id) { material += 1; snapshot = fixtureData(0); }
      return;
    }
    case 'extract_image': return new Promise<ImageExtraction>((resolve, reject) => {
      if (extractPending || translatePending || dialoguePending) { maximumConcurrentRequests = 2; reject('测试夹具：已有图片识别或翻译请求'); return; }
      maximumConcurrentRequests = Math.max(maximumConcurrentRequests, 1);
      extractPending = { resolve, reject, request: clone((args as { request: { dataUrl: string; name: string } }).request) };
    });
    case 'hide_to_tray': return;
    case 'plan_translation': return fixturePlan((args as { request: TranslationRequest }).request);
    case 'translate': return new Promise<TranslationResult>((resolve, reject) => {
      if (translatePending || extractPending || dialoguePending) { maximumConcurrentTranslations = 2; maximumConcurrentRequests = 2; reject('测试夹具：已有请求'); return; }
      maximumConcurrentTranslations = Math.max(maximumConcurrentTranslations, 1);
      maximumConcurrentRequests = Math.max(maximumConcurrentRequests, 1);
      translatePending = { resolve, reject, request: clone((args as { request: TranslationRequest }).request) };
    });
    case 'confirm_memory': return new Promise<void>((resolve, reject) => {
      rememberPending = { resolve, reject, entry: clone((args as { entry: MemoryInput }).entry) };
    });
    case 'new_session': return new Promise<string>((resolve, reject) => { sessionPending = { resolve, reject }; });
    case 'delete_history': {
      const id = (args as { id: string }).id;
      snapshot.sessionTurns = snapshot.sessionTurns.filter((turn) => turn.id !== id);
      snapshot.history = snapshot.history.filter((turn) => turn.id !== id);
      snapshot.contextId = uuid(20000 + serial++);
      saveCurrentFixtureConversation();
      return;
    }
    case 'save_settings': snapshot.settings = clone((args as { settings: Settings }).settings); return;
    case 'reset_context': snapshot.contextId = uuid(30000 + serial++); return snapshot.contextId;
    default: throw new Error(`Unexpected fixture IPC: ${command}`);
  }
}, { shouldMockEvents: true });

// Important: bridge.desktop must be evaluated only after mockIPC and isTauri.
const { default: TranslationWorkspace } = await import('../../src/TranslationWorkspace');
const { api } = await import('../../src/bridge');

const fixture = {
  get state() { return clone(snapshot); },
  get calls() { return clone(calls); },
  get maximumConcurrentTranslations() { return maximumConcurrentTranslations; },
  get maximumConcurrentRequests() { return maximumConcurrentRequests; },
  get capturePreferences() { return clone(capturePreferences); },
  get captureStarts() { return captureStarts; },
  get captureRecording() { return captureRecording; },
  async emitCaptureStatus(active: boolean, error = '') {
    capturePreferences = { ...capturePreferences, active, error };
    await emit('capture-status', clone(capturePreferences));
  },
  failNextShortcutSave() { rejectShortcutSave = true; },
  async emitCaptureImage(dataUrl: string, name = 'synthetic-capture.png') { await emit('capture-ready', { dataUrl, name }); },
  async emitCaptureError(message = '测试截屏错误：选区不可用，请重试。') { await emit('capture-error', message); },
  get pending() { return { dialogue: !!dialoguePending, translate: !!translatePending, remember: !!rememberPending, session: !!sessionPending, extract: !!extractPending }; },
  get lastDialogueRequest() { return clone(dialoguePending?.request ?? null); },
  setNextDialogueRoute(route: Partial<FixtureDialogueRoute>) { nextDialogueRoute = clone(route); },
  failNextDialogueRoute() { rejectNextDialogueRoute = true; },
  deferNextDialogueRoute() { deferNextDialogueRoute = true; },
  resolveDialogue(fail = false) {
    if (!dialoguePending) throw new Error('No dialogue route waiting');
    const pending = dialoguePending; dialoguePending = null;
    if (fail) pending.reject('测试语言识别失败：请重试。');
    else pending.resolve(acceptFixtureRoute(pending.route));
  },
  get lastRequest() { return clone(translatePending?.request ?? null); },
  get lastMemory() { return clone(rememberPending?.entry ?? null); },
  get lastImageRequest() { return clone(extractPending?.request ?? null); },
  get archivedConversations() { return clone(conversationSummaries('', 0)); },
  async reset(size = 85) {
    if (translatePending || rememberPending || sessionPending || extractPending || dialoguePending) throw new Error('Resolve pending fixture IPC before reset');
    archivedConversations.clear(); workspaceSnapshots.clear();
    material += 1; snapshot = fixtureData(size); calls.length = 0; maximumConcurrentTranslations = 0; maximumConcurrentRequests = 0; await publish?.();
  },
  resolveTranslate(fail = false) {
    if (!translatePending) throw new Error('No translate IPC waiting');
    const pending = translatePending; translatePending = null;
    if (fail) { pending.reject('测试网络错误：这一段未完成，请重试。'); return; }
    const row: TranslationResult = { ...pending.request, id: `fixture-response-${serial++}`, translation: `测试译文：${pending.request.text}`,
      detectedLanguage: pending.request.sourceLanguage === 'auto' ? 'en' : pending.request.sourceLanguage,
      origin: 'model', verified: true, createdAt: new Date(Date.UTC(2026, 9, 20, 10, 0, serial)).toISOString() };
    snapshot.sessionTurns = [...snapshot.sessionTurns, row];
    snapshot.history = snapshot.sessionTurns.slice(-200).reverse();
    saveCurrentFixtureConversation();
    pending.resolve(clone(row));
  },
  resolveImage(fail = false, text = 'Read this image as translation source only. <img src=x onerror=alert(1)>') {
    if (!extractPending) throw new Error('No image extraction IPC waiting');
    const pending = extractPending; extractPending = null;
    if (fail) { pending.reject('测试图片识别失败：请重试，原图与草稿已保留。'); return; }
    pending.resolve({ text, sourceImage: { id: `fixture-image-${serial++}`, name: pending.request.name,
      mimeType: 'image/png', thumbnailDataUrl: pending.request.dataUrl } });
  },
  resolveRemember(fail = false) {
    if (!rememberPending) throw new Error('No memory IPC waiting');
    const pending = rememberPending; rememberPending = null;
    if (fail) pending.reject('测试审核未通过'); else pending.resolve();
  },
  resolveSession() {
    if (!sessionPending) throw new Error('No session IPC waiting');
    const pending = sessionPending; sessionPending = null;
    saveCurrentFixtureConversation();
    const mode = snapshot.workspaceMode;
    material += 1; snapshot = fixtureData(0, mode); pending.resolve(snapshot.sessionId);
  },
  async removeLastAndRefresh() {
    const id = snapshot.sessionTurns.at(-1)?.id;
    if (id) await api.deleteHistory(id);
    await publish?.(); return id;
  },
  async resetContextAndRefresh() { await api.resetContext(); await publish?.(); },
};
Object.assign(window, { __moyuFixture: fixture });

function Harness() {
  const [data, setData] = useState<Bootstrap>(() => clone(snapshot));
  const [notice, setNotice] = useState('');
  const refresh = useCallback(async () => { const next = await api.bootstrap(); setData(next); return next; }, []);
  publish = refresh;
  return <div className="app-shell">
    <aside className="sidebar"><div className="brand"><span className="brand-symbol">译</span><span className="brand-name">墨译<span>LOCAL UI FIXTURE</span></span></div><nav><button className="nav-item active">连续翻译</button><button className="nav-item" onClick={() => void fixture.reset(85)}>重载 85 段</button><button className="nav-item" onClick={() => void fixture.reset(1000)}>测试 1,000 段</button><button className="nav-item" onClick={() => void fixture.removeLastAndRefresh()}>删除最后一段并刷新</button></nav><div className="sidebar-bottom"><p style={{ fontSize: 12, lineHeight: 1.8, color: '#596775' }}>全部文本为确定性测试数据。<br />此页面不调用模型、不访问真实数据库。</p></div></aside>
    <main className="main-content"><strong data-fixture-banner style={{ position: 'fixed', left: 14, bottom: 12, zIndex: 100, fontSize: 11, color: '#9a4c17' }} title={notice || '真实 TranslationWorkspace + 官方 mockIPC'}>本地界面测试 · 不连接模型</strong><div className="page-content translation-page">
      <TranslationWorkspace active data={data} loaded mutating={false} invalidationVersion={0} loadedDraft={null}
        onRefresh={refresh} onSettings={async (patch) => { await api.saveSettings({ ...snapshot.settings, ...patch }); await refresh(); }}
        onNotice={setNotice} onSettingsPage={() => setNotice('本地测试夹具：没有真实隐私设置')} />
    </div></main>
  </div>;
}
const mount = createRoot(document.getElementById('root')!);
if (new URLSearchParams(window.location.search).has('application')) {
  // Exercise real application chrome (including its tray control), not a test
  // copy of that control. This remains the isolated mockIPC test entry.
  snapshot = fixtureData(0);
  if (isHistoryFixture) seedConversationHistory();
  const { default: App } = await import('../../src/App');
  mount.render(<><App /><strong data-fixture-banner style={{ position: 'fixed', left: 14, bottom: 12, zIndex: 100, fontSize: 11, color: '#9a4c17' }}>本地应用测试 · 不连接模型</strong></>);
} else mount.render(<Harness />);

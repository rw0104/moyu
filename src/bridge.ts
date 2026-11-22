import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Bootstrap, CapturePreferences, ConversationPage, DialogueInput, DialogueRoute, Language, WorkspaceMode, ImageInput, ImageExtraction, MemoryInput, Settings, TermInput, TranslationPlan, TranslationRequest, TranslationResult } from './types';

export const desktop = isTauri();
const preview: Bootstrap = {
  settings: { model: 'deepseek-flash', style: 'natural', useMemory: true, useContext: true, saveHistory: true },
  hasApiKey: false, history: [], sessionTurns: [], conversations: [], hasMoreConversations: false, memories: [], terms: [], dataDirectory: '仅桌面版保存本地数据', sessionId: 'preview', contextId: 'preview',
  contextLimits: { maxTurns: 12, maxCharacters: 24_000, transcriptLimit: 0 },
  workspaceMode: 'standard', dialoguePreferences: { peerLanguage: 'auto', lastPeerLanguage: 'en', lastDirection: 'inbound' },
};
export const api = {
  bootstrap: () => desktop ? invoke<Bootstrap>('get_bootstrap') : Promise.resolve(structuredClone(preview)),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  saveApiKey: (apiKey: string) => invoke<void>('save_api_key', { apiKey }),
  removeApiKey: () => invoke<void>('remove_api_key'),
  translate: (request: TranslationRequest) => invoke<TranslationResult>('translate', { request }),
  extractImage: (request: ImageInput) => invoke<ImageExtraction>('extract_image', { request }),
  resolveDialogue: (request: DialogueInput) => invoke<DialogueRoute>('resolve_dialogue', { request }),
  saveDialoguePreferences: (peerLanguage: Language) => invoke<Bootstrap>('save_dialogue_preferences', { peerLanguage }),
  switchWorkspace: (mode: WorkspaceMode) => desktop ? invoke<Bootstrap>('switch_workspace', { mode }) : Promise.resolve({ ...structuredClone(preview), workspaceMode: mode, sessionId: `preview-${mode}` }),
  capturePreferences: () => desktop ? invoke<CapturePreferences>('get_capture_preferences') : Promise.resolve({ shortcut: 'Ctrl+Alt+T', active: false }),
  setCaptureShortcut: (shortcut: string) => invoke<CapturePreferences>('set_capture_shortcut', { shortcut }),
  setCaptureRecording: (recording: boolean) => invoke<void>('set_capture_recording', { recording }),
  startCapture: () => invoke<void>('start_capture'),
  conversations: (query: string, offset = 0) => desktop ? invoke<ConversationPage>('list_conversations', { query, offset }) : Promise.resolve({ conversations: [], hasMore: false }),
  openConversation: (id: string) => invoke<Bootstrap>('open_conversation', { id }),
  deleteConversation: (id: string) => invoke<void>('delete_conversation', { id }),
  planTranslation: (request: TranslationRequest) => invoke<TranslationPlan>('plan_translation', { request }),
  hideToTray: () => invoke<void>('hide_to_tray'),
  confirmMemory: (entry: MemoryInput) => invoke<void>('confirm_memory', { entry }),
  deleteMemory: (id: string) => invoke<void>('delete_memory', { id }),
  clearMemories: () => invoke<void>('clear_memories'),
  upsertTerm: (term: TermInput) => invoke<void>('upsert_term', { term }),
  deleteTerm: (id: string) => invoke<void>('delete_term', { id }),
  clearHistory: () => invoke<void>('clear_history'),
  deleteHistory: (id: string) => invoke<void>('delete_history', { id }),
  newSession: () => invoke<string>('new_session'),
  sessionTurns: () => desktop ? invoke<TranslationResult[]>('get_session_turns') : Promise.resolve([]),
  resetContext: () => invoke<string>('reset_context'),
};

export const listenCaptureReady = (handler: (image: ImageInput) => void) => desktop
  ? listen<ImageInput>('capture-ready', (event) => handler(event.payload)) : Promise.resolve(() => {});
export const listenCaptureError = (handler: (error: string) => void) => desktop
  ? listen<string>('capture-error', (event) => handler(event.payload)) : Promise.resolve(() => {});

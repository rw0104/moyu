export const LANGUAGES = [
  { code: 'auto', label: '自动检测' },
  { code: 'zh-CN', label: '简体中文' },
  { code: 'zh-TW', label: '繁體中文' },
  { code: 'en', label: '英语' },
  { code: 'ja', label: '日语' },
  { code: 'ko', label: '韩语' },
  { code: 'fr', label: '法语' },
  { code: 'de', label: '德语' },
  { code: 'es', label: '西班牙语' },
  { code: 'ru', label: '俄语' },
  { code: 'pt', label: '葡萄牙语' },
  { code: 'ar', label: '阿拉伯语' },
  { code: 'it', label: '意大利语' },
] as const;
export type Language = typeof LANGUAGES[number]['code'];
export type Style = 'faithful' | 'natural' | 'formal';
export type Model = 'deepseek-flash' | 'deepseek-v4-pro';
export type WorkspaceMode = 'standard' | 'dialogue';
export type DialogueDirection = 'auto' | 'inbound' | 'outbound';
export interface DialoguePreferences { peerLanguage: Language; lastPeerLanguage: Language; lastDirection: 'inbound' | 'outbound' }
export interface DialogueInput { text: string; sessionId: string; peerLanguage: Language; direction: DialogueDirection }
export interface DialogueRoute { sourceLanguage: Language; targetLanguage: Language; peerLanguage: Language; direction: 'inbound' | 'outbound'; usedFallback: boolean }
export interface Settings { model: Model; style: Style; useMemory: boolean; useContext: boolean; saveHistory: boolean }
export interface SourceImage { id: string; name: string; mimeType: string; thumbnailDataUrl: string }
export interface ImageInput { dataUrl: string; name: string }
export interface CapturePreferences { shortcut: string; active: boolean; error?: string }
export interface CaptureRequest extends ImageInput { id: string }
export interface ImageExtraction { text: string; sourceImage: SourceImage }
export interface ConversationSummary { id: string; title: string; preview: string; turnCount: number; createdAt: string; updatedAt: string; mode?: WorkspaceMode }
export interface ConversationPage { conversations: ConversationSummary[]; hasMore: boolean }
export interface TranslationRequest { text: string; sourceLanguage: Language; targetLanguage: Language; style: Style; sessionId: string; batchId?: string; sourceImage?: SourceImage }
export interface TranslationPlan { segments: string[]; estimatedInputTokens: number; outputTokenBudget: number }
export interface TranslationResult extends TranslationRequest {
  id: string; translation: string; detectedLanguage: string;
  origin: 'model' | 'memory' | 'literal'; verified: boolean; createdAt: string;
}
export interface MemoryInput {
  sourceText: string; translation: string; sourceLanguage: Language;
  targetLanguage: Language; style: Style;
}
export interface MemoryEntry extends MemoryInput { id: string; createdAt: string; model: string; glossaryRevision: number }
export interface TermInput { source: string; target: string; sourceLanguage: Language; targetLanguage: Language }
export interface Term extends TermInput { id: string }
export interface ContextLimits { maxTurns: number; maxCharacters: number; transcriptLimit: number }
export interface Bootstrap { settings: Settings; hasApiKey: boolean; history: TranslationResult[]; sessionTurns: TranslationResult[]; conversations: ConversationSummary[]; hasMoreConversations: boolean; memories: MemoryEntry[]; terms: Term[]; dataDirectory: string; sessionId: string; contextId: string; contextLimits: ContextLimits; workspaceMode?: WorkspaceMode; dialoguePreferences?: DialoguePreferences }

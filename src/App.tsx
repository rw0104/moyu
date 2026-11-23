import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  ArrowRight, BookMarked, BookOpen, Check, CheckCheck,
  ChevronDown, ChevronRight, Database, Feather, FileClock,
  Globe2, KeyRound, Languages, LoaderCircle, LockKeyhole, Pencil, Plus,
  Search, Settings2, ShieldCheck, Sparkles, Trash2, X, MessageSquareText,
} from 'lucide-react';
import { api, desktop, listenCaptureError, listenCaptureReady, listenCaptureStatus } from './bridge';
import CaptureSettings from './CaptureSettings';
import { DEFAULT_CAPTURE_SHORTCUT } from './capture-shortcut';
import TranslationWorkspace, { type LoadedDraft } from './TranslationWorkspace';
import { LANGUAGES, type Bootstrap, type CapturePreferences, type CaptureRequest, type ConversationPage, type Language, type MemoryEntry, type Model,
  type Settings, type Style, type Term, type TermInput, type TranslationResult, type WorkspaceMode } from './types';

type Page = 'translate' | 'dialogue' | 'memory' | 'glossary' | 'history' | 'settings';
const initialSettings: Settings = { model: 'deepseek-flash', style: 'natural', useMemory: true, useContext: true, saveHistory: true };
const languageLabel = (code: string) => LANGUAGES.find((language) => language.code === code)?.label ?? code;
const styleLabel: Record<Style, string> = { natural: '自然流畅', faithful: '忠实原文', formal: '正式书面' };
const modelLabel: Record<Model, string> = { 'deepseek-flash': 'DeepSeek Flash', 'deepseek-v4-pro': 'DeepSeek V4 Pro' };
const dateLabel = (value: string) => {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
};
const errorLabel = (error: unknown) => error instanceof Error ? error.message : String(error);

function LanguageSelect({ value, onChange, label, options = LANGUAGES, disabled = false }: {
  value: Language; onChange: (value: Language) => void; label: string;
  options?: readonly { code: Language; label: string }[]; disabled?: boolean;
}) {
  return <div className="language-select"><select aria-label={label} value={value} onChange={(event) => onChange(event.target.value as Language)} disabled={disabled}>
    {options.map((language) => <option key={language.code} value={language.code}>{language.label}</option>)}
  </select><ChevronDown size={14} aria-hidden="true" /></div>;
}

function Toggle({ checked, onChange, label, disabled = false }: {
  checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean;
}) {
  return <button className={`toggle ${checked ? 'is-on' : ''}`} role="switch" aria-checked={checked} aria-label={label}
    disabled={disabled} onClick={() => onChange(!checked)}><span /></button>;
}

function EmptyState({ icon, title, description }: { icon: ReactNode; title: string; description: string }) {
  return <div className="empty-state"><span className="empty-icon">{icon}</span><h3>{title}</h3><p>{description}</p></div>;
}

export default function App() {
  const [page, setPage] = useState<Page>('translate');
  const [data, setData] = useState<Bootstrap>({ settings: initialSettings, hasApiKey: false, memories: [], terms: [], history: [], conversations: [], hasMoreConversations: false, dataDirectory: '', sessionId: '', sessionTurns: [], contextId: '', contextLimits: { maxTurns: 12, maxCharacters: 24000, transcriptLimit: 0 } });
  const [loaded, setLoaded] = useState(false);
  const [loadedDraft, setLoadedDraft] = useState<LoadedDraft | null>(null);
  const [invalidationVersion, setInvalidationVersion] = useState(0);
  const [mutating, setMutating] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [search, setSearch] = useState('');
  const [conversationPage, setConversationPage] = useState<ConversationPage>({ conversations: [], hasMore: false });
  const [historyOffset, setHistoryOffset] = useState(0);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [capturePreferences, setCapturePreferences] = useState<CapturePreferences>({ shortcut: DEFAULT_CAPTURE_SHORTCUT, active: false });
  const [capturePreferencesLoading, setCapturePreferencesLoading] = useState(true);
  const captureStatusRevision = useRef(0);
  const [captureRequests, setCaptureRequests] = useState<CaptureRequest[]>([]);
  const [apiKey, setApiKey] = useState('');
  const [termForm, setTermForm] = useState<TermInput | null>(null);
  const [editingTerm, setEditingTerm] = useState<Term | null>(null);
  const [confirmAction, setConfirmAction] = useState<{ title: string; description: string; action: () => Promise<void> } | null>(null);
  const termSourceRef = useRef<HTMLInputElement>(null);
  const dialogCancelRef = useRef<HTMLButtonElement>(null);
  const refreshSequence = useRef(0);
  const historySequence = useRef(0);
  const settings = data.settings;
  const workspaceMode: WorkspaceMode = data.workspaceMode ?? 'standard';
  const workspaceModeRef = useRef<WorkspaceMode>(workspaceMode);
  workspaceModeRef.current = workspaceMode;
  const initialized = useRef(false);
  const translationPage = page === 'translate' || page === 'dialogue';

  const refresh = useCallback(async () => {
    const sequence = ++refreshSequence.current;
    const next = await api.bootstrap();
    if (sequence === refreshSequence.current) {
      setData(next); setLoaded(true);
      if (!initialized.current) { initialized.current = true; setPage(next.workspaceMode === 'dialogue' ? 'dialogue' : 'translate'); }
    }
    return next;
  }, []);

  useEffect(() => { refresh().catch((reason) => { setError(errorLabel(reason)); setLoaded(true); }); }, [refresh]);
  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    const keep = (stop: () => void) => { if (disposed) stop(); else stops.push(stop); };
    void listenCaptureReady((image) => {
      if (disposed) return;
      if (!image || typeof image.dataUrl !== 'string' || !image.dataUrl.startsWith('data:image/png;base64,') || typeof image.name !== 'string') {
        setError('截图内容无效，请重新截屏。'); return;
      }
      setCaptureRequests((current) => [...current, { ...image, id: crypto.randomUUID() }]);
      setPage(workspaceModeRef.current === 'dialogue' ? 'dialogue' : 'translate'); setSearch(''); setError('');
    }).then(keep).catch((reason) => { if (!disposed) setError(errorLabel(reason)); });
    void listenCaptureError((reason) => { if (!disposed) setError(reason); }).then(keep).catch((reason) => { if (!disposed) setError(errorLabel(reason)); });
    void listenCaptureStatus((status) => {
      if (disposed) return;
      captureStatusRevision.current++;
      setCapturePreferences(status);
    }).then(keep).catch((reason) => { if (!disposed) setError(errorLabel(reason)); });
    return () => { disposed = true; for (const stop of stops) stop(); };
  }, []);
  useEffect(() => {
    let disposed = false;
    const revision = captureStatusRevision.current;
    const isCurrent = () => !disposed && revision === captureStatusRevision.current;
    setCapturePreferencesLoading(true);
    void api.capturePreferences().then((next) => { if (isCurrent()) setCapturePreferences(next); })
      .catch((reason) => { if (isCurrent()) setCapturePreferences((current) => ({ ...current, active: false, error: errorLabel(reason) })); })
      .finally(() => { if (!disposed) setCapturePreferencesLoading(false); });
    return () => { disposed = true; };
  }, [page === 'settings']);
  const acceptCapture = useCallback((id: string) => setCaptureRequests((current) => current.filter((image) => image.id !== id)), []);
  async function saveCaptureShortcut(shortcut: string) {
    const next = await api.setCaptureShortcut(shortcut);
    captureStatusRevision.current++;
    setCapturePreferences(next);
    setNotice(shortcut ? '截屏快捷键已保存' : '截屏快捷键已关闭');
  }
  useEffect(() => {
    const sequence = ++historySequence.current;
    if (page !== 'history') return;
    setHistoryLoading(true);
    const timer = window.setTimeout(() => {
      void api.conversations(search.trim(), historyOffset).then((next) => {
        if (historySequence.current === sequence) setConversationPage(next);
      }).catch((reason) => { if (historySequence.current === sequence) setError(errorLabel(reason)); })
        .finally(() => { if (historySequence.current === sequence) setHistoryLoading(false); });
    }, search ? 200 : 0);
    return () => { window.clearTimeout(timer); };
  }, [page, search, historyOffset, data.conversations]);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(''), 3500);
    return () => window.clearTimeout(timer);
  }, [notice]);
  useEffect(() => { if (termForm) termSourceRef.current?.focus(); }, [Boolean(termForm)]);
  useEffect(() => { if (confirmAction) dialogCancelRef.current?.focus(); }, [confirmAction]);
  useEffect(() => {
    if (!termForm && !confirmAction) return;
    const previousFocus = document.activeElement as HTMLElement | null;
    const trapFocus = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const dialog = document.querySelector<HTMLElement>('[aria-modal="true"]');
      const controls = dialog?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex="0"]');
      if (!controls?.length) return;
      const first = controls[0]; const last = controls[controls.length - 1];
      if (event.shiftKey && (document.activeElement === first || !dialog?.contains(document.activeElement))) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || !dialog?.contains(document.activeElement))) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', trapFocus);
    return () => { document.removeEventListener('keydown', trapFocus); previousFocus?.focus(); };
  }, [Boolean(termForm), Boolean(confirmAction)]);

  async function mutate(action: () => Promise<unknown>, message: string) {
    if (!desktop) { setError('当前是浏览器预览。请启动墨译桌面版以使用此功能。'); return; }
    setMutating(true);
    setError('');
    try { await action(); await refresh(); setNotice(message); }
    catch (reason) { setError(errorLabel(reason)); }
    finally { setMutating(false); }
  }

  async function updateSettings(patch: Partial<Settings>) {
    setInvalidationVersion((value) => value + 1);
    await mutate(() => api.saveSettings({ ...settings, ...patch }), '偏好已保存');
  }

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape' && !mutating) { setConfirmAction(null); setTermForm(null); setEditingTerm(null); } };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [mutating]);

  function reuse(entry: MemoryEntry | TranslationResult) {
    setLoadedDraft({ text: 'sourceText' in entry ? entry.sourceText : entry.text, sourceLanguage: entry.sourceLanguage, targetLanguage: entry.targetLanguage, nonce: Date.now() });
    setPage(workspaceMode === 'dialogue' ? 'dialogue' : 'translate'); setSearch(''); setNotice('已载入原文，可编辑后翻译为新的一段');
  }

  function navigate(value: Page) {
    if (value === 'translate' || value === 'dialogue') {
      const mode: WorkspaceMode = value === 'dialogue' ? 'dialogue' : 'standard';
      if (mode !== workspaceMode) {
        if (workspaceBusy || mutating || captureRequests.length) { setError('当前材料正在处理，完成后可切换模式。'); return; }
        setMutating(true); setError('');
        void api.switchWorkspace(mode).then((next) => {
          ++refreshSequence.current; initialized.current = true;
          setData(next); setLoaded(true); setLoadedDraft(null); setPage(value); setSearch(''); setHistoryOffset(0);
        }).catch((reason) => setError(errorLabel(reason))).finally(() => setMutating(false));
        return;
      }
    }
    setPage(value); setError(''); setSearch(''); setHistoryOffset(0);
  }
  async function openConversation(id: string) {
    if (!desktop || workspaceBusy || mutating) return;
    setMutating(true); setError('');
    try {
      const next = await api.openConversation(id);
      ++refreshSequence.current;
      setData(next); setLoaded(true); setLoadedDraft(null); setPage(next.workspaceMode === 'dialogue' ? 'dialogue' : 'translate'); setSearch(''); setHistoryOffset(0);
    } catch (reason) { setError(errorLabel(reason)); }
    finally { setMutating(false); }
  }
  const normalizedSearch = search.trim().toLocaleLowerCase();
  const matches = (...values: string[]) => !normalizedSearch || values.some((value) => value.toLocaleLowerCase().includes(normalizedSearch));
  const memories = data.memories.filter((entry) => matches(entry.sourceText, entry.translation));
  const terms = data.terms.filter((entry) => matches(entry.source, entry.target));
  const navigation = [
    { id: 'translate' as const, label: '连续翻译', icon: Languages },
    { id: 'dialogue' as const, label: '双向对话', icon: MessageSquareText },
    { id: 'memory' as const, label: '翻译记忆', icon: BookMarked, count: data.memories.length },
    { id: 'glossary' as const, label: '术语库', icon: BookOpen, count: data.terms.length },
    { id: 'history' as const, label: '历史记录', icon: FileClock },
  ];
  const titles: Record<Page, { title: string; subtitle: string; eyebrow: string }> = {
    translate: { title: '让语言，自然相通。', subtitle: '专注每一句原文，留住每一种准确的表达。', eyebrow: 'YOUR WORDS, IN A NEW WORLD' },
    dialogue: { title: '双向对话', subtitle: '自动识别来往语言，专注和客户沟通。', eyebrow: 'A CONVERSATION WITHOUT LANGUAGE BARRIERS' },
    memory: { title: '翻译记忆', subtitle: '由你确认的译文会成为下一次翻译的参考。', eyebrow: 'YOUR PERSONAL TRANSLATION MEMORY' },
    glossary: { title: '术语库', subtitle: '为专有名词约定译法，让表达始终如一。', eyebrow: 'A VOCABULARY THAT IS YOURS' },
    history: { title: '历史记录', subtitle: '按会话保存完整译文，随时打开，接着翻译。', eyebrow: 'WORDS YOU HAVE TRAVELLED WITH' },
    settings: { title: '设置', subtitle: '连接模型、管理偏好，让翻译顺手一点。', eyebrow: 'MAKE YOURSELF AT HOME' },
  };

  return <div className="app-shell">
    <aside className="sidebar">
      <button className="brand" onClick={() => navigate('translate')} aria-label="墨译首页"><span className="brand-symbol"><Feather size={25} strokeWidth={1.5} /></span><span className="brand-name">墨译<span>MOYU TRANSLATE</span></span></button>
      <span className="nav-label">工作空间 <span>WORKSPACE</span></span>
      <nav aria-label="主导航">{navigation.map(({ id, label, icon: Icon, count }) => <button key={id} className={`nav-item ${page === id ? 'active' : ''}`} aria-current={page === id ? 'page' : undefined} onClick={() => navigate(id)}>
        <Icon size={19} strokeWidth={1.7} /><span>{label}</span>{count !== undefined && count > 0 && <span className="nav-count">{count}</span>}{page === id && <span className="nav-active-dot" />}
      </button>)}</nav>
      <div className="sidebar-bottom"><div className="sidebar-purpose"><ShieldCheck size={15} /><span>专注翻译，保留你的表达。</span></div>
        <button className={`nav-item settings-link ${page === 'settings' ? 'active' : ''}`} onClick={() => navigate('settings')} aria-current={page === 'settings' ? 'page' : undefined}><Settings2 size={19} strokeWidth={1.7} /><span>设置</span><ChevronRight size={15} /></button>
        <button className="connection-card" onClick={() => navigate('settings')}><span className={`status-dot ${data.hasApiKey ? 'connected' : ''}`} /><span><strong>{modelLabel[settings.model]}</strong><small>{desktop ? data.hasApiKey ? 'API Key 已配置' : '等待连接 · 配置 API Key' : '浏览器预览模式'}</small></span><ChevronRight size={14} /></button>
        <div className="sidebar-footer"><span>墨译桌面版</span><span>v0.6.0</span></div>
      </div>
    </aside>

    <main className="main-content">
      {!translationPage && <header className="topbar"><div className="breadcrumb">工作空间 <ChevronRight size={13} /><strong>{navigation.find((item) => item.id === page)?.label ?? '设置'}</strong></div><span className="topbar-security"><ShieldCheck size={15} />专注翻译 · 记忆在本地</span></header>}
      <div className={`page-content ${translationPage ? 'translation-page' : ''}`}>
        {!translationPage && <div className="page-heading"><div><h1>{titles[page].title}</h1><p className="page-subtitle">{titles[page].subtitle}</p></div></div>}
        {!desktop && !translationPage && <div className="preview-banner"><Globe2 size={16} /><span>当前为界面预览。翻译、记忆与密钥管理请在桌面版中使用。</span><span className="preview-tag">PREVIEW</span></div>}
        {error && <div className="error-banner" role="alert"><ShieldCheck size={17} /><span>{error}</span><button className="icon-button" aria-label="关闭提示" onClick={() => setError('')}><X size={16} /></button></div>}

        <TranslationWorkspace mode={workspaceMode} active={translationPage} data={data} loaded={loaded} mutating={mutating} invalidationVersion={invalidationVersion} loadedDraft={loadedDraft} onRefresh={refresh} onSettings={updateSettings} onNotice={setNotice} onSettingsPage={() => navigate('settings')} onBusyChange={setWorkspaceBusy} captureRequests={captureRequests} onCaptureAccepted={acceptCapture} captureShortcut={capturePreferences.active ? capturePreferences.shortcut : ''} />

        {(page === 'memory' || page === 'history' || page === 'glossary') && <>
          <div className="collection-toolbar"><div className="search-field"><Search size={16} /><input aria-label={page === 'history' ? '搜索会话' : '搜索原文或译文'} placeholder={page === 'glossary' ? '搜索术语或译法…' : page === 'history' ? '搜索会话中的原文或译文…' : '搜索原文或译文…'} value={search} onChange={(event) => { setSearch(event.target.value); setHistoryOffset(0); }} />{search && <button className="icon-button" aria-label="清空搜索" onClick={() => { setSearch(''); setHistoryOffset(0); }}><X size={14} /></button>}</div><div className="collection-actions"><span className="collection-count">{page === 'history' ? `本页 ${conversationPage.conversations.length} 份会话` : `共 ${page === 'memory' ? data.memories.length : data.terms.length} 条`}</span>{page === 'glossary' ? <button className="primary-button" disabled={!desktop || mutating} onClick={() => { setEditingTerm(null); setTermForm({ source: '', target: '', sourceLanguage: 'en', targetLanguage: 'zh-CN' }); }}><Plus size={16} />添加术语</button> : <button className="text-button danger" disabled={!desktop || mutating || (page === 'history' && workspaceBusy) || !(page === 'memory' ? data.memories.length : data.conversations.length)} onClick={() => setConfirmAction({ title: page === 'memory' ? '清空全部翻译记忆？' : '清空全部历史记录？', description: '此操作会删除本机保存的记录，删除后无法恢复。', action: async () => { await mutate(page === 'memory' ? api.clearMemories : api.clearHistory, '记录已清空'); setConfirmAction(null); } })}><Trash2 size={14} />清空{page === 'memory' ? '记忆' : '历史'}</button>}</div></div>
          {page === 'memory' && (memories.length ? <div className="entry-list">{memories.map((entry) => <article className="translation-entry" key={entry.id}><div className="entry-meta"><span><BookMarked size={13} />{languageLabel(entry.sourceLanguage)}<ArrowRight size={12} />{languageLabel(entry.targetLanguage)}</span><span>{styleLabel[entry.style]}<span className="meta-dot">·</span>{dateLabel(entry.createdAt)}</span></div><div className="entry-columns"><p>{entry.sourceText}</p><p>{entry.translation}</p></div><div className="entry-footer"><span><Check size={12} />已确认的翻译记忆</span><div><button className="text-button" onClick={() => reuse(entry)}>使用原文 <ArrowRight size={13} /></button><button className="icon-button danger" aria-label="删除这条翻译记忆" disabled={mutating} onClick={() => setConfirmAction({ title: '删除这条翻译记忆？', description: '删除后，这条译文将不再参与后续翻译。', action: async () => { await mutate(() => api.deleteMemory(entry.id), '记忆已删除'); setConfirmAction(null); } })}><Trash2 size={15} /></button></div></div></article>)}</div> : <EmptyState icon={<BookMarked size={31} strokeWidth={1.2} />} title={search ? '没有找到相关记忆' : '让第一句好译文留下来'} description={search ? '换个关键词试试。' : '完成翻译后，点击「校验并记住」，建立属于你的翻译记忆。'} />)}
          {page === 'history' && <>
            {workspaceBusy && <p className="history-busy-hint" role="status">当前材料正在处理。完成后可打开或删除其他会话。</p>}
            {historyLoading ? <div className="history-loading" role="status"><LoaderCircle size={18} className="spin" />正在读取会话…</div> : conversationPage.conversations.length ? <div className="conversation-history-list">{conversationPage.conversations.map((conversation) => <article className="history-conversation" key={conversation.id}>
              <div className="history-conversation-heading"><h2>{conversation.title || '未命名会话'}</h2>{conversation.mode === 'dialogue' && <span className="history-current-badge">双向对话</span>}{conversation.id === data.sessionId && <span className="history-current-badge">当前会话</span>}<time dateTime={conversation.updatedAt}>{dateLabel(conversation.updatedAt)}</time></div>
              <p className="history-conversation-preview" dir="auto">{conversation.preview}</p>
              <div className="history-conversation-footer"><span>{conversation.turnCount.toLocaleString('zh-CN')} 段翻译</span><div><button className="text-button" disabled={!desktop || mutating || workspaceBusy} onClick={() => void openConversation(conversation.id)}>打开会话 <ArrowRight size={13} /></button><button className="icon-button danger" aria-label={`删除会话 ${conversation.title}`} disabled={!desktop || mutating || workspaceBusy} onClick={() => setConfirmAction({ title: '删除这份会话？', description: '这份会话的全部原文、译文和图片缩略图将从本机删除，已确认的翻译记忆会保留。', action: async () => { await mutate(() => api.deleteConversation(conversation.id), '会话已删除'); setConfirmAction(null); } })}><Trash2 size={15} /></button></div></div>
            </article>)}</div> : <EmptyState icon={<FileClock size={31} strokeWidth={1.2} />} title={search ? '没有找到相关会话' : '从第一份会话开始'} description={search ? '可以搜索会话中的任意原文或译文。' : '开启历史保存后，每份材料的完整会话会保存在这里。'} />}
            {(historyOffset > 0 || conversationPage.hasMore) && <nav className="history-pagination" aria-label="会话分页"><button className="secondary-button" aria-label="上一页会话" disabled={historyLoading || historyOffset === 0} onClick={() => setHistoryOffset((value) => Math.max(0, value - 40))}>上一页</button><span>第 {Math.floor(historyOffset / 40) + 1} 页</span><button className="secondary-button" aria-label="下一页会话" disabled={historyLoading || !conversationPage.hasMore} onClick={() => setHistoryOffset((value) => value + 40)}>下一页</button></nav>}
          </>}
          {page === 'glossary' && (terms.length ? <div className="glossary-table"><div className="glossary-table-header"><span>原文术语</span><span>指定译法</span><span>翻译方向</span><span className="sr-only">操作</span></div>{terms.map((term) => <div className="glossary-row" key={term.id}><strong>{term.source}</strong><span>{term.target}</span><span className="term-direction">{languageLabel(term.sourceLanguage)}<ArrowRight size={11} />{languageLabel(term.targetLanguage)}</span><div><button className="icon-button" aria-label={`编辑术语 ${term.source}`} disabled={mutating} onClick={() => { setEditingTerm(term); setTermForm({ source: term.source, target: term.target, sourceLanguage: term.sourceLanguage, targetLanguage: term.targetLanguage }); }}><Pencil size={15} /></button><button className="icon-button danger" aria-label={`删除术语 ${term.source}`} disabled={mutating} onClick={() => void mutate(() => api.deleteTerm(term.id), '术语已删除')}><Trash2 size={15} /></button></div></div>)}</div> : <EmptyState icon={<BookOpen size={31} strokeWidth={1.2} />} title={search ? '没有找到相关术语' : '从一个专有名词开始'} description={search ? '换个关键词试试。' : '添加品牌名、产品名或专业词汇，为它们指定统一的译法。'} />)}
          <p className="collection-footnote"><Database size={13} />{page === 'memory' ? '记忆按翻译方向与风格匹配；模型、术语变化可能使既有记忆重新校验。' : page === 'glossary' ? '匹配的术语会作为翻译参考发送给 DeepSeek；修改术语后，已有记忆会重新评估。' : '历史记录保存在本机，未加密。删除历史不会同时删除已确认的翻译记忆。'}</p>
        </>}

        {page === 'settings' && <div className="settings-sections">
          <CaptureSettings preferences={capturePreferences} loading={capturePreferencesLoading} onSave={saveCaptureShortcut} />
          <section className="settings-card"><div className="settings-card-heading"><span className="settings-heading-icon"><KeyRound size={19} /></span><div><h2>连接 DeepSeek</h2><p>使用你自己的 API Key，费用由 DeepSeek 账户结算。</p></div><span className={`key-status ${data.hasApiKey ? 'configured' : ''}`}><span className={`status-dot ${data.hasApiKey ? 'connected' : ''}`} />{data.hasApiKey ? '已配置' : '未配置'}</span></div><form onSubmit={(event) => { event.preventDefault(); const key = apiKey.trim(); if (!key) return; void mutate(async () => { await api.saveApiKey(key); setApiKey(''); }, 'API Key 已保存'); }}><label className="field-label" htmlFor="api-key">API Key</label><div className="key-input-row"><div className="key-input"><KeyRound size={16} /><input id="api-key" type="password" autoComplete="new-password" spellCheck={false} value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={data.hasApiKey ? '已安全保存，输入新的 Key 以替换' : '输入 DeepSeek API Key'} disabled={!desktop || mutating} /></div><button className="primary-button" disabled={!desktop || mutating || !apiKey.trim()} type="submit">{mutating ? <LoaderCircle size={15} className="spin" /> : <Check size={15} />}保存密钥</button></div></form><div className="key-help"><p><LockKeyhole size={12} />密钥由系统凭据服务保管，界面不会读取或显示已保存的密钥。</p>{data.hasApiKey && <button className="text-button danger" disabled={mutating} onClick={() => setConfirmAction({ title: '移除 API Key？', description: '移除后需要重新配置密钥，才能继续使用 DeepSeek 翻译。', action: async () => { await mutate(api.removeApiKey, 'API Key 已移除'); setConfirmAction(null); } })}>移除密钥</button>}</div></section>
          <section className="settings-card"><div className="settings-card-heading"><span className="settings-heading-icon"><Sparkles size={19} /></span><div><h2>翻译偏好</h2><p>找到适合你的速度与表达方式。</p></div></div><div className="setting-row"><div><h3>翻译模型</h3><p>Flash 适合日常翻译，Pro 适合复杂语义。</p></div><div className="select-wrapper setting-select"><select aria-label="翻译模型" value={settings.model} disabled={!desktop || mutating} onChange={(event) => void updateSettings({ model: event.target.value as Model })}><option value="deepseek-flash">DeepSeek Flash</option><option value="deepseek-v4-pro">DeepSeek V4 Pro</option></select><ChevronDown size={14} /></div></div><div className="setting-row"><div><h3>默认翻译风格</h3><p>忠实保留原意，或让表达更自然、正式。</p></div><div className="select-wrapper setting-select"><select aria-label="默认翻译风格" value={settings.style} disabled={!desktop || mutating} onChange={(event) => void updateSettings({ style: event.target.value as Style })}>{Object.entries(styleLabel).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select><ChevronDown size={14} /></div></div><div className="setting-row"><div><h3>使用翻译记忆</h3><p>优先参考你确认过的译文，保留一致的表达。</p></div><Toggle checked={settings.useMemory} label="设置：使用翻译记忆" disabled={!desktop || mutating} onChange={(value) => void updateSettings({ useMemory: value })} /></div><div className="setting-row"><div><h3>连续上下文翻译</h3><p>接续同一材料最近 12 组翻译（合计最多 24,000 字符），辅助统一称谓与表达。</p></div><Toggle checked={settings.useContext} label="设置：连续上下文翻译" disabled={!desktop || mutating} onChange={(value) => void updateSettings({ useContext: value })} /></div><div className="setting-row"><div><h3>保存翻译历史</h3><p>开启后，完整会话保存在本机，可从历史记录重新打开并继续。</p></div><Toggle checked={settings.saveHistory} label="保存翻译历史" disabled={!desktop || mutating} onChange={(value) => void updateSettings({ saveHistory: value })} /></div></section>
          <section className="settings-card privacy-card"><div className="settings-card-heading"><span className="settings-heading-icon"><ShieldCheck size={19} /></span><div><h2>翻译边界与数据</h2><p>清楚知道，每一段文字如何被处理。</p></div></div><div className="privacy-detail"><CheckCheck size={17} /><div><h3>翻译专用 · 强制校验</h3><p>原文被作为待翻译内容处理。译文必须经过独立检查，未通过则不展示；此检查始终启用。模型仍可能出错，请核对重要译文。</p></div></div><div className="privacy-detail"><Globe2 size={17} /><div><h3>需要联网翻译</h3><p>原文、相关记忆与匹配术语会发送至 DeepSeek。图片由 DeepSeek Flash 识别并校验文字，再用所选模型翻译。连续上下文开启时，会同时发送同一材料最近 12 组翻译（合计最多 24,000 字符）。「新建上下文」可开始一份新材料。</p></div></div><div className="privacy-detail"><Database size={17} /><div><h3>记忆留在本机</h3><p>会话、术语和翻译记忆保存在本地数据库，未加密。图片只保存缩略图和识别文字，不保存原始图片。关闭历史保存后，新译文仅在本次运行期间暂存。密钥由系统凭据服务单独保存。</p><code className="data-path">{data.dataDirectory || '正在读取本地数据位置…'}</code></div></div></section>
        </div>}
        {!translationPage && <footer className="page-footer"><span>MOYU<span className="footer-dot">·</span>为准确表达而生</span><span>POWERED BY DEEPSEEK <span className="footer-line" /></span></footer>}
      </div>
    </main>
    {notice && <div className="toast" role="status"><Check size={16} />{notice}</div>}
    {termForm && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !mutating) { setTermForm(null); setEditingTerm(null); } }}><section className="modal" role="dialog" aria-modal="true" aria-labelledby="term-dialog-title"><div className="modal-header"><div><span className="eyebrow">YOUR VOCABULARY</span><h2 id="term-dialog-title">{editingTerm ? '编辑术语' : '添加新术语'}</h2></div><button className="icon-button" aria-label="关闭术语编辑" disabled={mutating} onClick={() => { setTermForm(null); setEditingTerm(null); }}><X size={19} /></button></div><form onSubmit={(event) => { event.preventDefault(); if (!termForm.source.trim() || !termForm.target.trim()) return; const submitted = { ...termForm, source: termForm.source.trim(), target: termForm.target.trim() }; void mutate(async () => { await api.upsertTerm(submitted); setTermForm(null); setEditingTerm(null); }, '术语已保存'); }}><div className="term-language-row"><LanguageSelect label="术语原文语言" value={termForm.sourceLanguage} options={LANGUAGES.filter((language) => language.code !== 'auto')} disabled={Boolean(editingTerm)} onChange={(value) => setTermForm({ ...termForm, sourceLanguage: value })} /><ArrowRight size={16} /><LanguageSelect label="术语译文语言" value={termForm.targetLanguage} options={LANGUAGES.filter((language) => language.code !== 'auto')} disabled={Boolean(editingTerm)} onChange={(value) => setTermForm({ ...termForm, targetLanguage: value })} /></div><label className="field-label" htmlFor="term-source">原文术语</label><input className="form-input" id="term-source" ref={termSourceRef} required value={termForm.source} disabled={Boolean(editingTerm)} onChange={(event) => setTermForm({ ...termForm, source: Array.from(event.target.value).slice(0, 120).join('') })} placeholder="例如：workspace" /><label className="field-label" htmlFor="term-target">指定译法</label><input className="form-input" id="term-target" required value={termForm.target} onChange={(event) => setTermForm({ ...termForm, target: Array.from(event.target.value).slice(0, 120).join('') })} placeholder="例如：工作空间" />{error && <p className="modal-error" role="alert">{error}</p>}<p className="form-help">{editingTerm ? '编辑指定译法；更换原文或翻译方向请新建术语。' : '翻译中遇到这个术语时，会参考你指定的译法。'}</p><div className="modal-actions"><button type="button" className="secondary-button" disabled={mutating} onClick={() => { setTermForm(null); setEditingTerm(null); }}>取消</button><button type="submit" className="primary-button" disabled={mutating || !termForm.source.trim() || !termForm.target.trim() || termForm.sourceLanguage === termForm.targetLanguage}>{mutating ? <LoaderCircle size={15} className="spin" /> : <Check size={15} />}保存术语</button></div>{termForm.sourceLanguage === termForm.targetLanguage && <p className="form-error" role="alert">原文和译文语言需要不同。</p>}</form></section></div>}
    {confirmAction && <div className="modal-backdrop"><section className="modal confirmation-modal" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-description"><span className="confirmation-icon"><Trash2 size={23} /></span><h2 id="confirm-title">{confirmAction.title}</h2><p id="confirm-description">{confirmAction.description}</p>{error && <p className="modal-error" role="alert">{error}</p>}<div className="modal-actions"><button ref={dialogCancelRef} className="secondary-button" disabled={mutating} onClick={() => setConfirmAction(null)}>取消</button><button className="danger-button" disabled={mutating} onClick={() => void confirmAction.action()}>{mutating ? <LoaderCircle size={15} className="spin" /> : <Trash2 size={15} />}确认删除</button></div></section></div>}
  </div>;
}

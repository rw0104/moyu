import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, ArrowRight, ArrowRightLeft, ArrowUp, BookMarked, Check, CheckCheck, ChevronDown,
  ChevronUp, Copy, ImagePlus, Languages, LoaderCircle, MessageSquareText, PanelBottomClose, Pause, Pencil, Play, Plus, RotateCcw,
  ScanLine, ShieldCheck, X } from 'lucide-react';
import { api, desktop } from './bridge';
import { IMAGE_ACCEPT, readImageFile, type ImageAttachment } from './image-input';
import TextContent from './TextContent';
import { LANGUAGES, type Bootstrap, type CaptureRequest, type Language, type Model, type Settings, type SourceImage, type Style, type TranslationRequest,
  type TranslationResult } from './types';

export interface LoadedDraft { text: string; sourceLanguage: Language; targetLanguage: Language; nonce: number }
interface WorkspaceProps {
  mode?: 'standard' | 'dialogue';
  active: boolean; data: Bootstrap; loaded: boolean; mutating: boolean; invalidationVersion: number;
  loadedDraft: LoadedDraft | null; onRefresh: () => Promise<Bootstrap>;
  onSettings: (patch: Partial<Settings>) => Promise<void>; onNotice: (message: string) => void;
  onSettingsPage: () => void;
  onBusyChange?: (busy: boolean) => void;
  captureRequests?: CaptureRequest[];
  onCaptureAccepted?: (id: string) => void;
  captureShortcut?: string;
}
const styles: Record<Style, string> = { natural: '自然流畅', faithful: '忠实原文', formal: '正式书面' };
const languageName = (code: string) => LANGUAGES.find((item) => item.code === code)?.label ?? code;
const count = (text: string) => Array.from(text).length;
const previewText = (text: string, length: number) => Array.from(text).slice(0, length).join('');
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const clock = (value: string) => new Date(value).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });

function LanguageControl({ value, onChange, label, options = LANGUAGES }: {
  value: Language; onChange: (value: Language) => void; label: string;
  options?: readonly { code: Language; label: string }[];
}) {
  return <span className="composer-select"><select aria-label={label} value={value} onChange={(event) => onChange(event.target.value as Language)}>
    {options.map((item) => <option value={item.code} key={item.code}>{item.label}</option>)}
  </select><ChevronDown size={12} aria-hidden="true" /></span>;
}

function SourceBlock({ text, sourceLanguage, image, label = '原文', compact = false }: { text: string; sourceLanguage: string; image?: Pick<SourceImage, 'name' | 'thumbnailDataUrl'>; label?: string; compact?: boolean }) {
  const long = compact || count(text) > 420 || text.split('\n').length > 6;
  return <div className={`turn-source ${long ? 'long-source' : ''}`}>
    {image && <div className="turn-source-image">{/^data:image\/(png|jpeg|webp|gif);base64,/i.test(image.thumbnailDataUrl) && <img src={image.thumbnailDataUrl} alt={image.name} />}<span>{image.name}<small>图片文字</small></span></div>}
    {long ? <details><summary><span className="source-label">{label} <span>{languageName(sourceLanguage)}</span></span><span className="source-peek" dir="auto">{previewText(text, 180)}</span><span className="source-expander">查看原文 <ChevronDown size={13} /></span></summary><p className="plain-text" dir="auto">{text}</p></details>
      : <><div className="source-label">{label} <span>{languageName(sourceLanguage)}</span></div><p className="plain-text" dir="auto">{text}</p></>}
  </div>;
}

function TranslationTurn({ turn, number, disabled, onRemember, onCopy, dialogue = false }: {
  turn: TranslationResult; number: number; disabled: boolean; dialogue?: boolean;
  onRemember: (turn: TranslationResult, translation: string) => Promise<void>; onCopy: (text: string) => void;
}) {
  const [translation, setTranslation] = useState(turn.translation);
  const [editing, setEditing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [remembered, setRemembered] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const long = count(translation) > 1600 || translation.split('\n').length > 15;
  const edited = translation !== turn.translation;
  const direction = turn.sourceLanguage.startsWith('zh') ? 'outbound' : 'inbound';

  async function remember() {
    setSaving(true); setError('');
    try { await onRemember(turn, translation); setRemembered(true); setEditing(false); }
    catch (reason) { setError(message(reason)); }
    finally { setSaving(false); }
  }
  return <article className={`conversation-turn ${dialogue ? `dialogue-turn is-${direction}` : ''}`} data-dialogue-direction={dialogue ? direction : undefined} aria-label={`第 ${number} 段翻译`}>
    <div className="turn-marker"><span>{String(number).padStart(2, '0')}</span><span className="turn-marker-line" /></div>
    <div className="turn-content"><div className="turn-metadata">{dialogue && <strong className={`dialogue-speaker is-${direction}`}>{direction === 'outbound' ? '我的消息' : '客户消息'}</strong>}<span>{languageName(turn.detectedLanguage || turn.sourceLanguage)}<ArrowRight size={12} />{languageName(turn.targetLanguage)}</span><span className="turn-style">{styles[turn.style]}</span><time dateTime={turn.createdAt}>{clock(turn.createdAt)}</time></div>
      <SourceBlock text={turn.text} sourceLanguage={turn.detectedLanguage || turn.sourceLanguage} image={turn.sourceImage} compact={dialogue} label={dialogue ? direction === 'outbound' ? '我的中文原文' : '客户原文' : '原文'} />
      <div className="turn-translation"><div className="translation-label"><span className="translation-avatar"><Languages size={15} /></span><strong>{dialogue ? direction === 'outbound' ? '发给客户的译文' : '中文译文' : '译文'}</strong>{edited ? <span className={`edited-badge ${remembered ? 'is-verified' : ''}`}>{remembered ? '已编辑，校验通过' : '已编辑，尚未校验'}</span> : <span className="validation-badge"><CheckCheck size={12} />{turn.origin === 'memory' ? '来自翻译记忆' : '校验通过'}</span>}</div>
        {editing ? <><label className="sr-only" htmlFor={`edit-${turn.id}`}>编辑第 {number} 段译文</label><textarea id={`edit-${turn.id}`} data-translation-editor="true" className="translation-editor" disabled={saving} dir="auto" value={translation} onChange={(event) => { setTranslation(event.target.value); setRemembered(false); }} /><div className="edit-toolbar"><span>按当前术语校验后，再存入翻译记忆。</span><button className="text-button" disabled={saving} onClick={() => setEditing(false)}><Check size={14} />完成编辑</button></div></> : <><TextContent text={translation} className={`plain-text translated-prose ${long && !expanded ? 'is-collapsed' : ''}`} />{long && <button className="expand-text-button" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? '收起译文' : '展开完整译文'}{expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}</button>}</>}
        {error && <p className="turn-error" role="alert">{error}</p>}
        <div className="turn-actions"><button className="turn-action" onClick={() => onCopy(translation)} disabled={!translation}><Copy size={14} />复制</button><button className={`turn-action ${editing ? 'selected' : ''}`} onClick={() => setEditing(!editing)} disabled={saving}><Pencil size={14} />编辑译文</button><button className={`turn-action remember-action ${remembered ? 'is-remembered' : ''}`} disabled={disabled || saving || !translation.trim() || remembered} onClick={() => void remember()}>{saving ? <LoaderCircle size={14} className="spin" /> : remembered ? <Check size={14} /> : <BookMarked size={14} />}{saving ? '正在校验' : remembered ? '已存入记忆' : '校验并记住'}</button></div>
      </div>
    </div>
  </article>;
}

interface FailedTurn { id: string; request: TranslationRequest; error: string; createdAt: string; batchId?: string; preserveDraft?: boolean }
interface CaptureJob extends CaptureRequest { recognized?: { text: string; sourceImage: SourceImage }; status: 'queued' | 'processing' | 'failed'; error?: string }
interface PendingTurn { resolving?: boolean; id: string; request: TranslationRequest; createdAt: string; image?: Pick<SourceImage, 'name' | 'thumbnailDataUrl'>; extracting?: boolean }
interface TranslationBatch {
  id: string; turnId: string; createdAt: string; request: TranslationRequest; segments: string[]; nextIndex: number;
  status: 'planning' | 'running' | 'pausing' | 'paused' | 'failed' | 'interrupted';
  config: string; error?: string; preserveDraft?: boolean;
}
const remainingSource = (batch: TranslationBatch) => batch.segments.length
  ? batch.segments.slice(batch.nextIndex).join('') : batch.request.text;

export default function TranslationWorkspace({ mode = 'standard', active, data, loaded, mutating, invalidationVersion, loadedDraft,
  onRefresh, onSettings, onNotice, onSettingsPage, onBusyChange, captureRequests = [], onCaptureAccepted, captureShortcut = '' }: WorkspaceProps) {
  const [draft, setDraft] = useState('');
  const [attachment, setAttachment] = useState<ImageAttachment | null>(null);
  const [readingImage, setReadingImage] = useState(false);
  const [extracting, setExtracting] = useState(false);
  const [source, setSource] = useState<Language>('auto');
  const [direction, setDirection] = useState<'auto' | 'inbound' | 'outbound'>('auto');
  const [savingPeer, setSavingPeer] = useState(false);
  const [routeNotice, setRouteNotice] = useState('');
  const [target, setTarget] = useState<Language>('zh-CN');
  const [sending, setSending] = useState(false);
  const [remembering, setRemembering] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [pending, setPending] = useState<PendingTurn | null>(null);
  const [batch, setBatch] = useState<TranslationBatch | null>(null);
  const [failures, setFailures] = useState<FailedTurn[]>([]);
  const [localTurns, setLocalTurns] = useState<TranslationResult[]>([]);
  const [error, setError] = useState('');
  const [hasNew, setHasNew] = useState(false);
  const [isAbove, setIsAbove] = useState(false);
  const [visibleCount, setVisibleCount] = useState(40);
  const [contextNotice, setContextNotice] = useState(false);
  const [captureJobs, setCaptureJobs] = useState<CaptureJob[]>([]);
  const [captureWorking, setCaptureWorking] = useState(false);
  const [startingCapture, setStartingCapture] = useState(false);
  const captureWorkingRef = useRef(false);
  const acceptedCaptures = useRef(new Set<string>());
  const scrollRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const imageInputRef = useRef<HTMLInputElement>(null);
  const imageReadSequence = useRef(0);
  const contextDetailsRef = useRef<HTMLDetailsElement>(null);
  const atBottomRef = useRef(true);
  const sendingRef = useRef(false);
  const batchRef = useRef<TranslationBatch | null>(null);
  const pauseRef = useRef(false);
  const resettingRef = useRef(false);
  const mountedRef = useRef(true);
  const sessionRef = useRef(data.sessionId);
  const contextRef = useRef(data.contextId);
  const peerLanguage = data.dialoguePreferences?.peerLanguage ?? 'auto';
  const learnedPeer = data.dialoguePreferences?.lastPeerLanguage ?? 'en';
  const config = `${mode}|${peerLanguage}|${data.sessionId}|${data.contextId}|${data.settings.model}|${data.settings.style}|${data.settings.useMemory}|${data.settings.useContext}|${data.settings.saveHistory}|${invalidationVersion}`;
  const configRef = useRef(config);
  configRef.current = config;
  const outbound = source.startsWith('zh');
  const inputCount = count(draft);
  const busy = sending || remembering || mutating || resetting || readingImage || savingPeer;
  const waitingCaptures = captureJobs.filter((job) => job.status === 'queued').length;
  const queued = batch !== null || captureWorking || waitingCaptures > 0;
  useEffect(() => { onBusyChange?.(sending || queued || remembering || resetting || readingImage || savingPeer); }, [sending, queued, remembering, resetting, readingImage, savingPeer, onBusyChange]);
  useEffect(() => {
    const incoming = captureRequests.filter((image) => !acceptedCaptures.current.has(image.id));
    if (!incoming.length) return;
    for (const image of incoming) acceptedCaptures.current.add(image.id);
    setCaptureJobs((current) => [...current, ...incoming.map((image) => ({ ...image, status: 'queued' as const }))]);
    for (const image of incoming) onCaptureAccepted?.(image.id);
  }, [captureRequests, onCaptureAccepted]);
  const updateBatch = useCallback((value: TranslationBatch | null) => {
    batchRef.current = value; setBatch(value);
  }, []);
  const turns = useMemo(() => {
    const map = new Map<string, TranslationResult>();
    for (const turn of [...data.sessionTurns, ...localTurns]) if (turn.sessionId === data.sessionId) map.set(turn.id, turn);
    return [...map.values()].sort((left, right) => left.createdAt.localeCompare(right.createdAt));
  }, [data.sessionTurns, data.sessionId, localTurns]);
  const readingWindowRef = useRef({ sessionId: data.sessionId, lastId: turns.at(-1)?.id, firstId: turns[Math.max(0, turns.length - visibleCount)]?.id });
  const previousMode = useRef(mode);
  const modeDrafts = useRef<Partial<Record<'standard' | 'dialogue', {
    draft: string; attachment: ImageAttachment | null; source: Language; target: Language;
    direction: 'auto' | 'inbound' | 'outbound'; failures: FailedTurn[]; captures: CaptureJob[];
  }>>>({});
  useLayoutEffect(() => {
    if (previousMode.current === mode) return;
    modeDrafts.current[previousMode.current] = { draft, attachment, source, target, direction, failures,
      captures: captureJobs.map((job) => job.status === 'processing' ? { ...job, status: 'failed', error: '模式已切换，截图保留，可重试。' } : job) };
    ++imageReadSequence.current; setReadingImage(false);
    const saved = modeDrafts.current[mode];
    setDraft(saved?.draft ?? ''); setAttachment(saved?.attachment ?? null);
    setSource(saved?.source ?? 'auto'); setTarget(saved?.target ?? 'zh-CN'); setDirection(saved?.direction ?? 'auto');
    setFailures(saved?.failures ?? []); setCaptureJobs(saved?.captures ?? []); setLocalTurns([]);
    setPending(null); setError(''); setRouteNotice(''); setHasNew(false); setVisibleCount(40); setContextNotice(false);
    sessionRef.current = data.sessionId; contextRef.current = data.contextId; previousMode.current = mode;
    atBottomRef.current = true;
  }, [mode]);


  useLayoutEffect(() => {
    const previous = readingWindowRef.current;
    const lastId = turns.at(-1)?.id;
    if (previous.sessionId === data.sessionId && previous.lastId !== lastId && !atBottomRef.current && previous.firstId) {
      const anchor = turns.findIndex((turn) => turn.id === previous.firstId);
      if (anchor >= 0) setVisibleCount((current) => Math.max(current, turns.length - anchor));
    }
    readingWindowRef.current = { sessionId: data.sessionId, lastId, firstId: turns[Math.max(0, turns.length - visibleCount)]?.id };
  }, [turns, visibleCount, data.sessionId]);

  useEffect(() => { mountedRef.current = true; return () => { mountedRef.current = false; }; }, []);
  useEffect(() => {
    if (sessionRef.current !== data.sessionId) {
      setLocalTurns([]); setFailures([]); setPending(null); setError(''); setDirection('auto'); setRouteNotice('');
      setHasNew(false); setVisibleCount(40); setContextNotice(false); atBottomRef.current = true;
    } else if (contextRef.current && contextRef.current !== data.contextId) setContextNotice(true);
    sessionRef.current = data.sessionId; contextRef.current = data.contextId;
  }, [data.sessionId, data.contextId]);
  useEffect(() => {
    setPending(null);
    const current = batchRef.current;
    if (current && current.config !== config) {
      pauseRef.current = true;
      updateBatch({ ...current, status: 'interrupted', error: '设置或上下文已更新，未译原文已保留。' });
      setFailures((values) => values.filter((item) => item.batchId !== current.id));
    }
  }, [config, updateBatch]);
  useEffect(() => {
    const authoritative = new Set(data.sessionTurns.map((turn) => turn.id));
    setLocalTurns((values) => values.some((turn) => authoritative.has(turn.id)) ? values.filter((turn) => !authoritative.has(turn.id)) : values);
  }, [data.sessionTurns]);
  useEffect(() => {
    if (!loadedDraft) return;
    setAttachment(null); setDraft(loadedDraft.text); setSource(loadedDraft.sourceLanguage); setTarget(loadedDraft.targetLanguage); setError('');
    requestAnimationFrame(() => composerRef.current?.focus());
  }, [loadedDraft]);
  useLayoutEffect(() => {
    const element = composerRef.current;
    if (element) { element.style.height = '0px'; element.style.height = `${Math.min(Math.max(element.scrollHeight, 44), 160)}px`; }
  }, [draft, active]);
  useEffect(() => {
    const closeContext = (event: PointerEvent | KeyboardEvent) => {
      const details = contextDetailsRef.current;
      if (details?.open && (event instanceof KeyboardEvent ? event.key === 'Escape' : !details.contains(event.target as Node))) {
        details.open = false;
        if (event instanceof KeyboardEvent) details.querySelector('summary')?.focus();
      }
    };
    document.addEventListener('pointerdown', closeContext);
    document.addEventListener('keydown', closeContext);
    return () => { document.removeEventListener('pointerdown', closeContext); document.removeEventListener('keydown', closeContext); };
  }, []);
  const latestItemKey = `${turns.map((turn) => turn.id).join(',')}|${pending?.id ?? ''}|${failures.map((turn) => turn.id).join(',')}`;
  useLayoutEffect(() => {
    if (!active) return;
    const region = scrollRef.current;
    if (!region) return;
    if (atBottomRef.current) { region.scrollTop = region.scrollHeight; setHasNew(false); setIsAbove(false); }
    else setHasNew(true);
  }, [latestItemKey, active]);

  function toLatest() {
    const element = scrollRef.current;
    if (element) element.scrollTop = element.scrollHeight;
    atBottomRef.current = true; setHasNew(false); setIsAbove(false);
  }
  function inspectScroll() {
    const element = scrollRef.current;
    if (!element) return;
    const nearBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 80;
    atBottomRef.current = nearBottom; setIsAbove(!nearBottom);
    if (nearBottom) setHasNew(false);
  }
  function loadEarlier() {
    const region = scrollRef.current;
    const previousHeight = region?.scrollHeight ?? 0;
    const previousTop = region?.scrollTop ?? 0;
    atBottomRef.current = false;
    setVisibleCount((value) => value + 40);
    requestAnimationFrame(() => { if (region) region.scrollTop = previousTop + region.scrollHeight - previousHeight; });
  }

  const runBatch = useCallback(async (initial: TranslationBatch, needsPlan: boolean) => {
    let job = initial;
    let request = job.request;
    let pendingId = job.turnId;
    sendingRef.current = true; setSending(true); setError(''); pauseRef.current = false;
    updateBatch(job);
    const isCurrent = () => mountedRef.current && configRef.current === job.config && batchRef.current?.id === job.id;
    try {
      if (needsPlan) {
        setPending({ id: pendingId, request, createdAt: job.createdAt });
        const plan = await api.planTranslation(request);
        if (!isCurrent()) return;
        if (!plan.segments.length || plan.segments.some((segment) => !segment.length) || plan.segments.join('') !== request.text) {
          throw new Error('原文分段未能完整保留，请重试。');
        }
        job = { ...job, segments: plan.segments, status: 'running' };
        updateBatch(job);
      }
      while (job.nextIndex < job.segments.length) {
        if (!isCurrent()) return;
        if (pauseRef.current) { updateBatch({ ...job, status: 'paused' }); return; }
        request = { ...job.request, text: job.segments[job.nextIndex], ...(job.segments.length > 1 ? { batchId: job.id } : {}) };
        pendingId = job.segments.length > 1 ? `${job.turnId}-${job.nextIndex}` : job.turnId;
        const createdAt = job.nextIndex === 0 ? job.createdAt : new Date().toISOString();
        setPending({ id: pendingId, request, createdAt });
        setFailures((values) => values.filter((item) => item.id !== pendingId && item.batchId !== job.id));
        const response = await api.translate(request);
        if (!isCurrent()) return;
        if (!response.verified) throw new Error('译文未通过翻译校验，请重试。');
        if (response.sessionId !== request.sessionId) throw new Error('上下文已变化，未显示这次译文。');
        setLocalTurns((values) => [...values.filter((item) => item.id !== response.id), response]);
        setPending(null);
        job = { ...job, nextIndex: job.nextIndex + 1, status: pauseRef.current ? 'pausing' : 'running' };
        updateBatch(job);
        try { await onRefresh(); } catch { setError('译文已完成，但列表刷新失败；当前译文仍可复制。'); }
      }
      if (isCurrent()) updateBatch(null);
    } catch (reason) {
      if (!isCurrent()) return;
      const failure: FailedTurn = { id: pendingId, request, createdAt: new Date().toISOString(), error: message(reason),
        ...(job.segments.length > 1 ? { batchId: job.id } : {}), preserveDraft: job.preserveDraft };
      setPending(null);
      setFailures((values) => [...values.filter((item) => item.id !== pendingId && item.batchId !== job.id), failure]);
      if (job.segments.length > 1) updateBatch({ ...job, status: 'failed', error: failure.error });
      else { updateBatch(null); if (!job.preserveDraft) setDraft((current) => current || job.request.text); }
    } finally {
      sendingRef.current = false;
      if (mountedRef.current) { setSending(false); setPending(null); }
    }
  }, [onRefresh, updateBatch]);

  const resumeBatch = useCallback(async () => {
    const job = batchRef.current;
    if (!job || sendingRef.current || resettingRef.current || remembering || mutating || job.config !== configRef.current
      || !['paused', 'failed'].includes(job.status)) return;
    await runBatch({ ...job, status: 'running', error: undefined }, false);
  }, [remembering, mutating, runBatch]);

  const send = useCallback(async (retry?: FailedTurn) => {
    if (retry?.batchId && batchRef.current?.id === retry.batchId) { await resumeBatch(); return; }
    let request: TranslationRequest = retry?.request ?? { text: draft, sourceLanguage: source, targetLanguage: target, style: data.settings.style, sessionId: data.sessionId };
    const imageSnapshot = retry ? null : attachment;
    if (sendingRef.current || batchRef.current || captureWorkingRef.current || waitingCaptures || resettingRef.current || remembering || mutating || savingPeer || readingImage || (!request.text.trim() && !imageSnapshot) || !loaded) return;
    if (request.sessionId !== data.sessionId) { setError('上下文已更新，请将这一段重新放入输入框。'); return; }
    if (!desktop) { setError('当前为浏览器预览。请启动桌面版以连接 DeepSeek 和使用本地翻译记忆。'); return; }
    const requestConfig = configRef.current;
    const id = crypto.randomUUID();
    const turnId = retry?.id ?? `pending-${id}`;
    const createdAt = retry?.createdAt ?? new Date().toISOString();
    const directionSnapshot = direction;
    sendingRef.current = true; setSending(true); setError(''); setRouteNotice(''); atBottomRef.current = true;
    try {
      if (imageSnapshot) {
        setExtracting(true); atBottomRef.current = true;
        setPending({ id: turnId, request, createdAt, image: { name: imageSnapshot.name, thumbnailDataUrl: imageSnapshot.dataUrl }, extracting: true });
        const extraction = await api.extractImage({ dataUrl: imageSnapshot.dataUrl, name: imageSnapshot.name });
        if (!mountedRef.current) return;
        if (requestConfig !== configRef.current) throw new Error('设置或上下文已更新，图片和原文仍保留在输入框中，请重新翻译。');
        if (!extraction.text.trim()) throw new Error('图片中没有识别到可翻译的文字，请换一张更清晰的图片。');
        request = { ...request, text: request.text.trim() ? `${request.text}\n\n${extraction.text}` : extraction.text, sourceImage: extraction.sourceImage };
        setExtracting(false);
      }
      if (mode === 'dialogue' && !retry) {
        setPending({ id: turnId, request, createdAt, resolving: true, image: request.sourceImage });
        const route = await api.resolveDialogue({ text: request.text, sessionId: request.sessionId, peerLanguage, direction: directionSnapshot });
        if (!mountedRef.current) return;
        if (requestConfig !== configRef.current) throw new Error('对话或偏好已更新，原文仍保留，请重新翻译。');
        request = { ...request, sourceLanguage: route.sourceLanguage, targetLanguage: route.targetLanguage };
        setRouteNotice(route.usedFallback
          ? `短句方向不明确，已按${languageName(route.sourceLanguage)} → ${languageName(route.targetLanguage)}处理；可在发送方向中调整。`
          : `${languageName(route.sourceLanguage)} → ${languageName(route.targetLanguage)} · 已自动选择方向`);
      }
      if (requestConfig !== configRef.current) throw new Error('对话或偏好已更新，原文仍保留，请重新翻译。');
      if (imageSnapshot) setAttachment((current) => current?.id === imageSnapshot.id ? null : current);
      setFailures((values) => values.filter((item) => item.id !== turnId));
      if (!retry?.preserveDraft && (!retry || draft === request.text)) setDraft((current) => current === draft ? '' : current);
      if (!retry && mode === 'dialogue') setDirection((current) => current === directionSnapshot ? 'auto' : current);
      atBottomRef.current = true;
      requestAnimationFrame(() => composerRef.current?.focus());
      await runBatch({ id, turnId, createdAt, request, segments: [], nextIndex: 0,
        status: 'planning', config: requestConfig, preserveDraft: retry?.preserveDraft }, true);
    } catch (reason) { if (mountedRef.current) setError(message(reason)); }
    finally {
      sendingRef.current = false;
      if (mountedRef.current) { setExtracting(false); setSending(false); setPending(null); }
    }
  }, [draft, attachment, source, target, direction, mode, peerLanguage, data.settings.style, data.sessionId, remembering, mutating, savingPeer, readingImage, loaded, waitingCaptures, resumeBatch, runBatch]);

  const processCapture = useCallback(async (image: CaptureJob) => {
    if (captureWorkingRef.current || sendingRef.current || batchRef.current || resettingRef.current) return;
    captureWorkingRef.current = true; setCaptureWorking(true);
    sendingRef.current = true; setSending(true); setExtracting(!image.recognized); setError(''); setRouteNotice('');
    setCaptureJobs((current) => current.map((job) => job.id === image.id ? { ...job, status: 'processing', error: undefined } : job));
    const requestConfig = configRef.current;
    const id = crypto.randomUUID(); const turnId = `capture-${image.id}`; const createdAt = new Date().toISOString();
    let request: TranslationRequest = { text: '', sourceLanguage: source, targetLanguage: target, style: data.settings.style, sessionId: data.sessionId };
    atBottomRef.current = true;
    setPending({ id: turnId, request, createdAt, image: { name: image.name, thumbnailDataUrl: image.dataUrl }, extracting: !image.recognized });
    try {
      const extraction = image.recognized ?? await api.extractImage({ dataUrl: image.dataUrl, name: image.name });
      if (!mountedRef.current) return;
      if (requestConfig !== configRef.current) throw new Error('设置或上下文已更新，截图仍保留，请重试。');
      if (!extraction.text.trim()) throw new Error('截图中没有识别到可翻译文字，请重试或重新截屏。');
      setCaptureJobs((current) => current.map((job) => job.id === image.id ? { ...job, recognized: extraction } : job));
      request = { ...request, text: extraction.text, sourceImage: extraction.sourceImage };
      setExtracting(false);
      if (mode === 'dialogue') {
        setPending({ id: turnId, request, createdAt, resolving: true, image: extraction.sourceImage });
        const route = await api.resolveDialogue({ text: request.text, sessionId: request.sessionId, peerLanguage, direction: 'auto' });
        if (!mountedRef.current) return;
        if (requestConfig !== configRef.current) throw new Error('对话或偏好已更新，截图保留，可重试。');
        request = { ...request, sourceLanguage: route.sourceLanguage, targetLanguage: route.targetLanguage };
        setRouteNotice(route.usedFallback ? `短句方向不明确，已按${languageName(route.sourceLanguage)} → ${languageName(route.targetLanguage)}处理；可在发送方向中调整。` : `${languageName(route.sourceLanguage)} → ${languageName(route.targetLanguage)} · 已自动选择方向`);
      }
      setCaptureJobs((current) => current.filter((job) => job.id !== image.id));
      await runBatch({ id, turnId, createdAt, request, segments: [], nextIndex: 0, status: 'planning', config: requestConfig, preserveDraft: true }, true);
    } catch (reason) {
      if (mountedRef.current) setCaptureJobs((current) => current.map((job) => job.id === image.id ? { ...job, status: 'failed', error: message(reason) } : job));
    } finally {
      captureWorkingRef.current = false; sendingRef.current = false;
      if (mountedRef.current) { setCaptureWorking(false); setSending(false); setExtracting(false); setPending(null); }
    }
  }, [source, target, mode, peerLanguage, data.settings.style, data.sessionId, runBatch]);

  useEffect(() => {
    if (!desktop || !loaded || busy || batch || captureWorking || captureWorkingRef.current || resettingRef.current) return;
    const image = captureJobs.find((job) => job.status === 'queued');
    if (image) void processCapture(image);
  }, [captureJobs, loaded, busy, batch, captureWorking, processCapture]);

  async function changePeerLanguage(value: Language) {
    if (!desktop || busy || queued) return;
    setSavingPeer(true); setError(''); setRouteNotice('');
    try { await api.saveDialoguePreferences(value); await onRefresh(); }
    catch (reason) { setError(message(reason)); }
    finally { if (mountedRef.current) setSavingPeer(false); }
  }

  async function startCapture() {
    if (!desktop || startingCapture) return;
    setStartingCapture(true); setError('');
    try { await api.startCapture(); }
    catch (reason) { setError(message(reason)); }
    finally { if (mountedRef.current) setStartingCapture(false); }
  }

  async function attachImage(file: File) {
    const sequence = ++imageReadSequence.current;
    setReadingImage(true); setError('');
    try {
      const image = await readImageFile(file);
      if (!mountedRef.current || sequence !== imageReadSequence.current) return;
      if (attachment) onNotice('已替换输入框中的图片');
      setAttachment(image);
      composerRef.current?.focus();
    } catch (reason) { if (sequence === imageReadSequence.current) setError(message(reason)); }
    finally { if (sequence === imageReadSequence.current) setReadingImage(false); }
  }

  function pauseBatch() {
    const job = batchRef.current;
    if (!job || !['running', 'pausing'].includes(job.status)) return;
    pauseRef.current = true;
    updateBatch({ ...job, status: sendingRef.current ? 'pausing' : 'paused' });
  }
  function retrieveRemaining() {
    const job = batchRef.current;
    if (!job || sendingRef.current) return;
    const remaining = remainingSource(job);
    setDraft((current) => current ? `${remaining}\n\n${current}` : remaining);
    setSource(job.request.sourceLanguage); setTarget(job.request.targetLanguage);
    setFailures((values) => values.filter((item) => item.batchId !== job.id));
    updateBatch(null); setError('');
    requestAnimationFrame(() => composerRef.current?.focus());
  }
  function restoreFailure(failure: FailedTurn) {
    if (failure.batchId && batchRef.current?.id === failure.batchId) { retrieveRemaining(); return; }
    setDraft((current) => current && current !== failure.request.text ? `${failure.request.text}\n\n${current}` : failure.request.text);
    setSource(failure.request.sourceLanguage); setTarget(failure.request.targetLanguage);
    requestAnimationFrame(() => composerRef.current?.focus());
  }
  async function hideToTray() {
    try { await api.hideToTray(); }
    catch (reason) { setError(message(reason)); }
  }

  useEffect(() => {
    const keyHandler = (event: KeyboardEvent) => {
      if (!active || event.isComposing || document.querySelector('[aria-modal="true"]')) return;
      if ((event.ctrlKey || event.metaKey) && event.key === 'Enter' && !(event.target as HTMLElement | null)?.closest('[data-translation-editor]')) {
        event.preventDefault(); void send();
      }
    };
    window.addEventListener('keydown', keyHandler);
    return () => window.removeEventListener('keydown', keyHandler);
  }, [active, send]);

  async function remember(turn: TranslationResult, translation: string) {
    setRemembering(true);
    try {
      await api.confirmMemory({ sourceText: turn.text, translation, sourceLanguage: turn.sourceLanguage, targetLanguage: turn.targetLanguage, style: turn.style });
      await onRefresh(); onNotice('译文已通过校验，存入翻译记忆');
    } finally { setRemembering(false); }
  }
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); onNotice('译文已复制'); }
    catch { setError('复制失败，请选中文字后使用 Ctrl+C。'); }
  }
  async function newSession() {
    if (!desktop || busy || queued || resettingRef.current) return;
    const previousDraft = draft;
    resettingRef.current = true; setResetting(true); setError(''); setDraft('');
    try { await api.newSession(); await onRefresh(); onNotice('已新建上下文，可以开始另一份材料'); composerRef.current?.focus(); }
    catch (reason) { setError(message(reason)); setDraft((current) => current || previousDraft); }
    finally { resettingRef.current = false; setResetting(false); }
  }
  const timeline = [
    ...turns.slice(-visibleCount).map((turn, index) => ({ kind: 'turn' as const, id: turn.id, createdAt: turn.createdAt, turn, number: Math.max(0, turns.length - visibleCount) + index + 1 })),
    ...failures.map((failure) => ({ kind: 'failure' as const, id: failure.id, createdAt: failure.createdAt, failure })),
    ...(pending ? [{ kind: 'pending' as const, id: pending.id, createdAt: pending.createdAt, pending }] : []),
  ].sort((left, right) => left.createdAt.localeCompare(right.createdAt));

  return <section className={`translation-workspace ${mode === 'dialogue' ? 'is-dialogue' : ''}`} aria-label={mode === 'dialogue' ? '双向对话工作台' : '连续翻译工作台'} hidden={!active}>
    <header className="conversation-header"><h1>{mode === 'dialogue' ? '双向对话' : '连续翻译'}</h1>{mode === 'dialogue' && <span className="dialogue-header-pair">中文 <ArrowRightLeft size={12} />{languageName(peerLanguage === 'auto' ? learnedPeer : peerLanguage)}</span>}<div className="workspace-header-actions"><details className="context-details" ref={contextDetailsRef}><summary title="上下文信息" aria-label="上下文信息"><span className={`context-dot ${data.settings.useContext ? 'enabled' : ''}`} /><span>上下文</span><ChevronDown size={12} /></summary><div className="context-popover"><strong>{data.settings.useContext ? mode === 'dialogue' ? '对话上下文' : '连续上下文' : '独立翻译'}</strong><p>{data.settings.useContext ? `模型参考最近 ${data.contextLimits.maxTurns} 组翻译，合计最多 ${data.contextLimits.maxCharacters.toLocaleString('zh-CN')} 字符。${mode === 'dialogue' ? '双向消息可接续同一对话，' : ''}更早记录仍可回看。` : '模型不参考先前段落，会话记录仍可回看。'}</p><span className="context-count">当前材料 {turns.length.toLocaleString('zh-CN')} 段{data.contextLimits.transcriptLimit > 0 ? ` / ${data.contextLimits.transcriptLimit.toLocaleString('zh-CN')} 段` : ' · 已保存记录不自动清理'}</span><button className="text-button" onClick={onSettingsPage}>翻译偏好 <ArrowRight size={12} /></button></div></details><button className="icon-button" title="收起到托盘" aria-label="收起到托盘" disabled={!desktop} onClick={() => void hideToTray()}><PanelBottomClose size={17} /></button><button className="icon-button new-session-button" title="新建上下文" aria-label="新建上下文" onClick={() => void newSession()} disabled={!desktop || busy || queued}>{resetting ? <LoaderCircle size={17} className="spin" /> : <Plus size={19} />}</button></div></header>
    <div className="conversation-region"><div className="conversation-scroll" ref={scrollRef} onScroll={inspectScroll} role="region" aria-label="当前上下文的翻译会话" tabIndex={0}>
      {contextNotice && <div className="context-boundary"><ShieldCheck size={13} /><span>翻译上下文已更新。先前译文保留，后续段落使用新的翻译偏好。</span><button className="icon-button" aria-label="关闭上下文更新提示" onClick={() => setContextNotice(false)}><X size={13} /></button></div>}
      {!loaded ? <div className="conversation-empty"><LoaderCircle size={26} className="spin" /><h2>正在读取你的工作空间</h2></div> : !timeline.length ? <div className="conversation-empty"><span className="empty-conversation-symbol"><MessageSquareText size={31} strokeWidth={1.4} /><span className="small-translation-symbol"><Languages size={15} /></span></span><h2>{mode === 'dialogue' ? '用熟悉的语言，聊下去。' : '从第一段开始。'}</h2><p>{mode === 'dialogue' ? <>输入中文，翻译给客户；粘贴客户消息，读懂对方。<br />自动识别方向，客户语言也会随对话接续。</> : <>粘贴文章、邮件或任何你想读懂的文字。<br />继续发送下一段，称谓与表达会接续前文。</>}</p><div className="empty-conversation-features"><span><ShieldCheck size={13} />翻译专用，逐段校验</span><span><BookMarked size={13} />你的译法，可以记住</span></div></div> : <div className="conversation-timeline">{turns.length > visibleCount ? <button className="load-earlier" onClick={loadEarlier}><ChevronUp size={14} />加载更早的 40 段<span>已显示最近 {Math.min(visibleCount, turns.length)} / {turns.length} 段</span></button> : <div className="timeline-start"><span />当前材料<span /></div>}{timeline.map((item) => item.kind === 'turn' ? <TranslationTurn key={item.id} turn={item.turn} number={item.number} disabled={busy || !desktop} onRemember={remember} onCopy={(text) => void copy(text)} dialogue={mode === 'dialogue'} /> : item.kind === 'pending' ? <article key={item.id} className="conversation-turn pending-turn" aria-busy="true"><div className="turn-marker"><LoaderCircle size={15} className="spin" /></div><div className="turn-content"><SourceBlock text={item.pending.request.text} sourceLanguage={item.pending.request.sourceLanguage} image={item.pending.image ?? item.pending.request.sourceImage} /><div className="pending-status" role="status"><LoaderCircle size={15} className="spin" /><span>{item.pending.extracting ? '正在识别图片文字' : item.pending.resolving ? '正在识别翻译方向' : '正在翻译并校验这一段'}<span>{item.pending.extracting ? '识别并核对图片中的可见文字' : item.pending.resolving ? '中文发给客户，客户消息译成中文' : '你可以继续准备下一段原文'}</span></span></div></div></article> : <article className="conversation-turn failed-turn" key={item.id}><div className="turn-marker"><RotateCcw size={15} /></div><div className="turn-content"><SourceBlock text={item.failure.request.text} sourceLanguage={item.failure.request.sourceLanguage} image={item.failure.request.sourceImage} /><div className="failure-message" role="alert"><strong>这段未完成翻译</strong><p>{item.failure.error}</p><div><button className="text-button" disabled={busy} onClick={() => void send(item.failure)}><RotateCcw size={14} />重试这一段</button><button className="text-button" disabled={busy} onClick={() => restoreFailure(item.failure)}><Pencil size={14} />放回输入框</button><button className="icon-button" aria-label="关闭这段失败提示" onClick={() => setFailures((values) => values.filter((value) => value.id !== item.id))}><X size={14} /></button></div></div></div></article>)}</div>}
    </div>{(isAbove || hasNew) && <button className="back-to-latest" onClick={toLatest}><ArrowDown size={14} />{hasNew ? '有新译文 · 回到最新' : '回到最新'}</button>}</div>
    <div className="composer-dock">{!desktop && <div className="workspace-preview"><span>浏览器预览</span>翻译与本地记忆请在桌面版中使用。</div>}{error && <div className="composer-error" role="alert"><span>{error}</span><button className="icon-button" aria-label="关闭错误提示" onClick={() => setError('')}><X size={14} /></button></div>}
      {batch && (batch.segments.length > 1 || batch.status === 'planning' || batch.status === 'interrupted') && <div className="translation-batch" aria-label="长文翻译进度" data-status={batch.status}>
        <div className="batch-summary"><span className="batch-indicator">{sending ? <LoaderCircle size={14} className="spin" /> : <Pause size={14} />}</span><div><strong>{batch.status === 'planning' ? '正在准备翻译' : `长文翻译 ${batch.nextIndex} / ${batch.segments.length || '—'}`}</strong><span role="status">{batch.status === 'planning' ? '保留完整原文，按需要自动分段' : batch.status === 'pausing' ? '完成当前段后暂停，已完成的译文会保留' : batch.status === 'paused' ? '已暂停，继续时从下一段开始' : batch.status === 'failed' ? '当前段未完成，重试后会接着翻译' : batch.status === 'interrupted' ? batch.error : data.settings.useContext ? '按原文顺序翻译，前后文自动接续' : '这篇长文的分段会接续上下文，完成后恢复独立翻译'}</span></div><div className="batch-actions">{['running', 'pausing'].includes(batch.status) ? <button className="text-button" aria-label="暂停长文翻译" disabled={batch.status === 'pausing'} onClick={pauseBatch}><Pause size={13} />{batch.status === 'pausing' ? '正在暂停' : '暂停'}</button> : ['paused', 'failed'].includes(batch.status) ? <button className="text-button" aria-label={batch.status === 'failed' ? '重试并继续长文翻译' : '继续长文翻译'} disabled={busy} onClick={() => void resumeBatch()}>{batch.status === 'failed' ? <RotateCcw size={13} /> : <Play size={13} />}{batch.status === 'failed' ? '重试并继续' : '继续翻译'}</button> : null}{['paused', 'failed', 'interrupted'].includes(batch.status) && <button className="text-button" disabled={busy} onClick={retrieveRemaining}>取回未译原文</button>}</div></div>
        {batch.segments.length > 1 && <progress className="batch-progress" value={batch.nextIndex} max={batch.segments.length} aria-label="已完成的翻译段数" />}
      </div>}
      {captureJobs.length > 0 && <div className="capture-queue" aria-label="截图翻译队列"><div className="capture-queue-title"><ScanLine size={14} /><strong>截图翻译</strong><span>{captureWorking ? '正在处理' : waitingCaptures ? `${waitingCaptures} 张等待翻译` : '识别未完成'}</span></div><div className="capture-queue-items">{captureJobs.map((job) => <div className={`capture-queue-item is-${job.status}`} key={job.id}><img src={job.dataUrl} alt={job.name} /><div><strong>{job.name}</strong><span role={job.status === 'failed' ? 'alert' : 'status'}>{job.status === 'processing' ? '正在识别并核对文字…' : job.status === 'queued' ? '等待当前任务完成后自动翻译' : job.error}</span></div>{job.status === 'failed' && <button className="text-button" aria-label={`重试截图翻译 ${job.name}`} disabled={busy || queued} onClick={() => setCaptureJobs((current) => current.map((value) => value.id === job.id ? { ...value, status: 'queued', error: undefined } : value))}><RotateCcw size={13} />重试</button>}{job.status !== 'processing' && <button className="icon-button" aria-label={`移除截图 ${job.name}`} onClick={() => setCaptureJobs((current) => current.filter((value) => value.id !== job.id))}><X size={13} /></button>}</div>)}</div></div>}
      {mode === 'dialogue' && routeNotice && <p className="dialogue-route-notice" role="status">{routeNotice}</p>}
      <div className="composer">{attachment && <div className="composer-attachment" aria-label="待翻译图片"><img src={attachment.dataUrl} alt={attachment.name} /><div><strong>{attachment.name}</strong><span title="图片会发送至 DeepSeek Flash 识别并校验，再由当前选择的模型翻译。原图不保存到历史，只保存缩略图和识别文字。">{extracting ? '正在识别 · 可继续准备下一段' : '图片文字 · 发送后识别并翻译'}</span></div><button className="icon-button" aria-label="移除图片" title="移除图片" onClick={() => { ++imageReadSequence.current; setAttachment(null); setReadingImage(false); }}><X size={14} /></button></div>}<input className="sr-only" ref={imageInputRef} type="file" accept={IMAGE_ACCEPT} aria-label="选择待翻译图片" tabIndex={-1} onChange={(event) => { const file = event.target.files?.[0]; event.target.value = ''; if (file) void attachImage(file); }} /><label className="sr-only" htmlFor="source-text">待翻译的原文</label><textarea id="source-text" ref={composerRef} rows={1} dir="auto" spellCheck={false} value={draft} onChange={(event) => setDraft(event.target.value)} onPaste={(event) => { const files = Array.from(event.clipboardData.files); if (files.length) { event.preventDefault(); if (files.length > 1) onNotice('每次翻译一张图片，已选择第一张'); void attachImage(files[0]); } }} placeholder={sending ? '继续准备下一段，上一段正在翻译…' : mode === 'dialogue' ? '输入中文，或粘贴客户消息…' : outbound ? '输入中文，翻译成另一种语言…' : '输入或粘贴原文…'} aria-describedby="composer-help" />
        <div className="composer-bottom"><div className="composer-controls"><button className="icon-button composer-capture" aria-label="截屏并翻译" title={captureShortcut ? `截屏并翻译（${captureShortcut}）` : '截屏并翻译'} disabled={!desktop || !loaded || startingCapture} onClick={() => void startCapture()}>{startingCapture ? <LoaderCircle size={16} className="spin" /> : <ScanLine size={17} />}</button><button className="icon-button composer-upload" title="上传图片，也可在输入框粘贴图片" aria-label="上传图片" onClick={() => imageInputRef.current?.click()}>{readingImage ? <LoaderCircle size={16} className="spin" /> : <ImagePlus size={17} />}</button>{mode === 'dialogue' ? <div className="dialogue-composer-controls"><span className="composer-select dialogue-peer"><select aria-label="客户语言" value={peerLanguage} disabled={!desktop || busy || queued} onChange={(event) => void changePeerLanguage(event.target.value as Language)}><option value="auto">自动跟随 · {languageName(learnedPeer)}</option>{LANGUAGES.filter((item) => item.code !== 'auto' && !item.code.startsWith('zh')).map((item) => <option key={item.code} value={item.code}>客户 · {item.label}</option>)}</select><ChevronDown size={12} /></span><span className="composer-select dialogue-direction"><select aria-label="对话翻译方向" value={direction} onChange={(event) => setDirection(event.target.value as 'auto' | 'inbound' | 'outbound')}><option value="auto">自动识别</option><option value="outbound">中文发给客户</option><option value="inbound">客户来信译中文</option></select><ChevronDown size={12} /></span></div> : <div className="composer-languages"><LanguageControl value={source} label="原文语言" onChange={(value) => { setSource(value); setTarget(value.startsWith('zh') ? 'en' : 'zh-CN'); }} /><button className="composer-swap" aria-label="交换翻译方向" title="交换翻译方向" onClick={() => { setSource(target); setTarget(source === 'auto' ? 'en' : source); }}><ArrowRightLeft size={14} /></button><LanguageControl value={target} label="目标语言" onChange={setTarget} options={outbound ? LANGUAGES.filter((item) => item.code !== 'auto' && item.code !== source) : LANGUAGES.filter((item) => item.code.startsWith('zh'))} /></div>}<span className="composer-control-divider" /><span className="composer-select composer-style"><select aria-label="翻译风格" value={data.settings.style} disabled={!desktop || busy} onChange={(event) => void onSettings({ style: event.target.value as Style })}>{Object.entries(styles).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select><ChevronDown size={12} /></span></div><div className="composer-submit-group"><span className="composer-status">{inputCount > 0 && <span className="composer-character-count" title="原文字符数">{inputCount.toLocaleString()} 字</span>}</span><span className="composer-select composer-model"><select aria-label="翻译模型" value={data.settings.model} disabled={!desktop || busy || queued} onChange={(event) => void onSettings({ model: event.target.value as Model })}><option value="deepseek-flash">DeepSeek Flash</option><option value="deepseek-v4-pro">DeepSeek V4 Pro</option></select><ChevronDown size={11} /></span>{draft && <button className="icon-button composer-clear" aria-label="清空输入框" title="清空输入框" onClick={() => setDraft('')}><X size={14} /></button>}<button className="composer-send" title="翻译（Ctrl + Enter）" aria-label="翻译这一段" disabled={!loaded || busy || queued || (!draft.trim() && !attachment)} onClick={() => void send()}>{sending ? <LoaderCircle size={17} className="spin" /> : <ArrowUp size={18} />}</button></div></div>
        <span className="sr-only" id="composer-help">按 Ctrl + Enter 翻译。可截屏、上传或粘贴图片。长文自动分段，提问与指令也只作为原文翻译。</span>
      </div>
    </div>
  </section>;
}

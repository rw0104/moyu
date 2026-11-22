import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Check, Keyboard, LoaderCircle, RotateCcw, ScanLine } from 'lucide-react';
import { api, desktop } from './bridge';
import type { CapturePreferences } from './types';

export const DEFAULT_CAPTURE_SHORTCUT = 'Ctrl+Alt+T';
const errorMessage = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function shortcutFromKey(event: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>): string | null {
  if ((!event.ctrlKey && !event.altKey) || event.metaKey) return null;
  const key = /^Key[A-Z]$/.test(event.code) ? event.code.slice(3)
    : /^Digit[0-9]$/.test(event.code) ? event.code.slice(5)
      : /^F([1-9]|1[0-9]|2[0-4])$/.test(event.code) ? event.code : '';
  const modifiers = [event.ctrlKey ? 'Ctrl' : '', event.altKey ? 'Alt' : '', event.shiftKey ? 'Shift' : ''].filter(Boolean);
  if (!key || key === 'F12' || (event.altKey && key === 'F4')) return null;
  return [...modifiers, key].join('+');
}

interface Props {
  preferences: CapturePreferences;
  loading: boolean;
  onSave: (shortcut: string) => Promise<void>;
}

export default function CaptureSettings({ preferences, loading, onSave }: Props) {
  const [draft, setDraft] = useState(preferences.shortcut);
  const [recording, setRecording] = useState(false);
  const [ready, setReady] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const fieldRef = useRef<HTMLInputElement>(null);
  const focusedRef = useRef(false);
  const mountedRef = useRef(true);
  const recordingOperation = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => { setDraft(preferences.shortcut); }, [preferences.shortcut]);

  function suspendShortcut(value: boolean) {
    const operation = recordingOperation.current.catch(() => {}).then(() => api.setCaptureRecording(value));
    recordingOperation.current = operation;
    return operation;
  }
  useEffect(() => {
    mountedRef.current = true;
    // WebView window deactivation need not blur the focused DOM input by itself.
    // End recording so returning to the window cannot leave a stale "ready" field.
    const endRecording = () => fieldRef.current?.blur();
    window.addEventListener('blur', endRecording);
    return () => { window.removeEventListener('blur', endRecording); mountedRef.current = false; focusedRef.current = false; if (desktop) void suspendShortcut(false).catch(() => {}); };
  }, []);

  async function save(shortcut: string) {
    setSaving(true); setError(''); fieldRef.current?.blur();
    try {
      await recordingOperation.current.catch(() => {});
      await onSave(shortcut);
      setDraft(shortcut);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { if (mountedRef.current) setSaving(false); }
  }
  function record(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Tab') return;
    if (event.key === 'Escape') {
      event.preventDefault(); event.stopPropagation(); setDraft(preferences.shortcut); setError(''); fieldRef.current?.blur(); return;
    }
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
    const shortcut = shortcutFromKey(event);
    if (!shortcut) { setError('请使用 Ctrl 或 Alt，配合字母、数字或 F1–F24（F12 除外）；可加 Shift。系统保留组合不可使用。'); return; }
    event.preventDefault(); event.stopPropagation();
    if (!ready) return;
    setDraft(shortcut); setError('');
  }

  return <section className="settings-card capture-settings" aria-labelledby="capture-settings-title">
    <div className="settings-card-heading"><span className="settings-heading-icon"><ScanLine size={19} /></span><div><h2 id="capture-settings-title">截屏翻译</h2><p>按下快捷键，框选文字，自动识别并翻译。</p></div><span className={`key-status ${preferences.active ? 'configured' : ''}`}><span className={`status-dot ${preferences.active ? 'connected' : ''}`} />{loading ? '正在读取' : preferences.active ? '全局快捷键已启用' : preferences.shortcut ? '快捷键未生效' : '快捷键已关闭'}</span></div>
    <label className="field-label" htmlFor="capture-shortcut">全局快捷键</label>
    <div className="capture-shortcut-row"><div className={`capture-shortcut-field ${recording ? 'is-recording' : ''}`}><Keyboard size={17} /><input id="capture-shortcut" ref={fieldRef} aria-label="截屏翻译快捷键" aria-describedby="capture-shortcut-help" readOnly disabled={!desktop || loading || saving} value={recording && !ready ? '' : draft} placeholder={recording ? '正在准备录入…' : '点击后按下组合键'} onKeyDown={record} onFocus={() => {
      focusedRef.current = true; setRecording(true); setReady(false); setError('');
      void suspendShortcut(true).then(() => { if (mountedRef.current && focusedRef.current) setReady(true); }).catch((reason) => { if (mountedRef.current) { setError(errorMessage(reason)); fieldRef.current?.blur(); } });
    }} onBlur={() => { focusedRef.current = false; setRecording(false); setReady(false); void suspendShortcut(false).catch((reason) => { if (mountedRef.current) setError(errorMessage(reason)); }); }} /></div><button className="primary-button" aria-label="保存截屏快捷键" disabled={!desktop || loading || saving || !draft || (draft === preferences.shortcut && preferences.active)} onClick={() => void save(draft)}>{saving ? <LoaderCircle size={15} className="spin" /> : <Check size={15} />}保存快捷键</button></div>
    <div className="capture-shortcut-footer"><p className="form-help" id="capture-shortcut-help">{recording ? ready ? '现在按下新组合键，Esc 取消。' : '正在暂停当前快捷键…' : '应用收起到托盘后仍可使用。框选完成即翻译，Esc 或右键取消。'}</p><div><button className="text-button" aria-label="恢复默认截屏快捷键" disabled={!desktop || loading || saving || (preferences.shortcut === DEFAULT_CAPTURE_SHORTCUT && preferences.active)} onClick={() => void save(DEFAULT_CAPTURE_SHORTCUT)}><RotateCcw size={13} />恢复默认</button><button className="text-button" aria-label="关闭截屏快捷键" disabled={!desktop || loading || saving || !preferences.shortcut} onClick={() => void save('')}>关闭快捷键</button></div></div>
    {(error || preferences.error) && <p className="form-error" role="alert">{error || preferences.error}</p>}
  </section>;
}

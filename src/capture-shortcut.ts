export const isMacOS = /\bMacintosh\b/.test(navigator.userAgent);
export const DEFAULT_CAPTURE_SHORTCUT = isMacOS ? 'Cmd+Shift+2' : 'Ctrl+Alt+T';

const macKeyNames: Record<string, string> = {
  Cmd: '⌘ Command', Ctrl: '⌃ Control', Alt: '⌥ Option', Shift: '⇧ Shift',
};

export function shortcutLabel(shortcut: string): string {
  if (!isMacOS) return shortcut;
  return shortcut.split('+').map((key) => macKeyNames[key] ?? key).join(' + ');
}

type ShortcutEvent = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>;

export function shortcutFromKey(event: ShortcutEvent): string | null {
  if (event.metaKey && !isMacOS) return null;
  if (!event.metaKey && !event.ctrlKey && !event.altKey) return null;
  // Option and Shift change the typed character. The native hotkey uses the physical key.
  const key = /^Key[A-Z]$/.test(event.code) ? event.code.slice(3)
    : /^Digit[0-9]$/.test(event.code) ? event.code.slice(5)
      : /^F([1-9]|1[0-9]|2[0-4])$/.test(event.code) ? event.code : '';
  if (!key) return null;
  if (isMacOS && /^F2[1-4]$/.test(key)) return null;
  if (!isMacOS && (key === 'F12' || (event.altKey && key === 'F4'))) return null;
  return [event.metaKey && 'Cmd', event.ctrlKey && 'Ctrl', event.altKey && 'Alt',
    event.shiftKey && 'Shift', key].filter(Boolean).join('+');
}

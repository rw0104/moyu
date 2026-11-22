import { useMemo } from 'react';

type Block = { kind: 'paragraph' | 'heading' | 'code'; text: string } | { kind: 'list'; items: Array<{ marker: string; text: string }> };

/** A small display parser. Content is rendered only through React text nodes, never interpreted as HTML. */
function blocks(text: string): Block[] {
  const result: Block[] = [];
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  let paragraph: string[] = [];
  const flush = () => { if (paragraph.length) { result.push({ kind: 'paragraph', text: paragraph.join('\n') }); paragraph = []; } };
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    if (/^\s*```[\w+-]*\s*$/.test(line)) {
      flush();
      const code: string[] = [];
      let closing = index + 1;
      while (closing < lines.length && !/^\s*```\s*$/.test(lines[closing])) code.push(lines[closing++]);
      if (closing < lines.length) { result.push({ kind: 'code', text: code.join('\n') }); index = closing; }
      else paragraph.push(line);
    } else if (!line.trim()) flush();
    else if (/^#{1,4}\s+\S/.test(line)) { flush(); result.push({ kind: 'heading', text: line.replace(/^#{1,4}\s+/, '') }); }
    else {
      const item = /^(\s*(?:[-*+]|\d+[.)]))\s+(.+)$/.exec(line);
      if (item) {
        flush();
        const previous = result.at(-1);
        const value = { marker: item[1].trim(), text: item[2] };
        if (previous?.kind === 'list') previous.items.push(value);
        else result.push({ kind: 'list', items: [value] });
      } else paragraph.push(line);
    }
  }
  flush();
  return result;
}

export default function TextContent({ text, className = '' }: { text: string; className?: string }) {
  const content = useMemo(() => blocks(text), [text]);
  return <div className={`readable-text ${className}`} dir="auto">{content.map((block, index) => block.kind === 'code'
    ? <pre className="text-code" key={index}><code>{block.text}</code></pre>
    : block.kind === 'heading' ? <div className="text-heading" key={index}>{block.text}</div>
      : block.kind === 'list' ? <ul className="text-list" key={index}>{block.items.map((item, position) => <li key={position}><span className="text-list-marker" aria-hidden="true">{item.marker}</span><span>{item.text}</span></li>)}</ul>
        : <p key={index}>{block.text}</p>)}</div>;
}

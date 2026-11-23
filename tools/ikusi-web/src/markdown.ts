import MarkdownIt from 'markdown-it';
import { nodeHref } from './navigation';

// Handle wiki references as Markdown tokens so fences, inline code and link destinations keep their literal text.
const md = new MarkdownIt({ html: false, linkify: true });
md.inline.ruler.before('link', 'wikilink', (state, silent) => {
  const match = /^\[\[([^\]\n|]+)(?:\|([^\]\n]+))?\]\]/.exec(state.src.slice(state.pos, state.posMax));
  if (!match) return false;
  if (!silent) {
    const id = match[1].trim();
    const known = state.env.ids?.has(id);
    const open = state.push(known ? 'link_open' : 'span_open', known ? 'a' : 'span', 1);
    open.attrSet('class', known ? 'wikilink' : 'missing-link');
    if (known) open.attrSet('href', nodeHref(id));
    state.push('text', '', 0).content = match[2] || id;
    state.push(known ? 'link_close' : 'span_close', known ? 'a' : 'span', -1);
  }
  state.pos += match[0].length;
  return true;
});
md.renderer.rules.link_open = (tokens, idx, options, _env, self) => {
  if (/^https?:/i.test(tokens[idx].attrGet('href') || '')) {
    tokens[idx].attrSet('target', '_blank');
    tokens[idx].attrSet('rel', 'noopener');
  }
  return self.renderToken(tokens, idx, options);
};
export function markdown(text: string, ids: Set<string> | undefined, title: string) {
  const env = { ids };
  const tokens = md.parse(text, env);
  if (
    tokens[0]?.type === 'heading_open' &&
    tokens[0].tag === 'h1' &&
    tokens[1]?.content.trim() === title.trim()
  ) {
    tokens.splice(0, 3);
  }
  return md.renderer.render(tokens, md.options, env);
}
export const inline = (text: string, ids: Set<string> | undefined) => md.renderInline(text, { ids });

//! Canonical-compatible JavaScript for the core browser differential lane.
//!
//! Keeping the DOM walkers out of `browser_control.rs` makes the control
//! dispatch readable while retaining the scripts as deterministic pure data.

use super::*;

pub(in crate::control_socket) fn browser_snapshot_script(
    params: &serde_json::Map<String, Value>,
) -> String {
    let interactive = bool_param(params, &["interactive"]).unwrap_or(false);
    let include_cursor = bool_param(params, &["cursor"]).unwrap_or(false);
    let compact = bool_param(params, &["compact"]).unwrap_or(false);
    let max_depth = usize_param(params, &["max_depth", "maxDepth"]).unwrap_or(12);
    let scope = string_param(params, &["selector"]);
    let scope = serde_json::to_string(&scope).expect("snapshot scope JSON is infallible");

    SNAPSHOT_TEMPLATE
        .replace(
            "__INTERACTIVE__",
            if interactive { "true" } else { "false" },
        )
        .replace("__CURSOR__", if include_cursor { "true" } else { "false" })
        .replace("__COMPACT__", if compact { "true" } else { "false" })
        .replace("__MAX_DEPTH__", &max_depth.to_string())
        .replace("__SCOPE__", &scope)
}

const SNAPSHOT_TEMPLATE: &str = r#"(() => {
  try {
    const __interactiveOnly = __INTERACTIVE__;
    const __includeCursor = __CURSOR__;
    const __compact = __COMPACT__;
    const __maxDepth = __MAX_DEPTH__;
    const __scopeSelector = __SCOPE__;
    const __normalize = (s) => String(s || '').replace(/\s+/g, ' ').trim();
    const __interactiveRoles = new Set(['button','link','textbox','checkbox','radio','combobox','listbox','menuitem','menuitemcheckbox','menuitemradio','option','searchbox','slider','spinbutton','switch','tab','treeitem']);
    const __contentRoles = new Set(['heading','cell','gridcell','columnheader','rowheader','listitem','article','region','main','navigation']);
    const __structuralRoles = new Set(['generic','group','list','table','row','rowgroup','grid','treegrid','menu','menubar','toolbar','tablist','tree','directory','document','application','presentation','none']);

    const __isVisible = (el) => {
      try {
        if (!el) return false;
        const style = getComputedStyle(el);
        const rect = el.getBoundingClientRect();
        if (!style || !rect || rect.width <= 0 || rect.height <= 0) return false;
        if (style.display === 'none' || style.visibility === 'hidden') return false;
        return parseFloat(style.opacity || '1') > 0.01;
      } catch (_) { return false; }
    };

    const __implicitRole = (el) => {
      const tag = String(el.tagName || '').toLowerCase();
      if (tag === 'button') return 'button';
      if (tag === 'a' && el.hasAttribute('href')) return 'link';
      if (tag === 'input') {
        const type = String(el.getAttribute('type') || 'text').toLowerCase();
        if (type === 'checkbox') return 'checkbox';
        if (type === 'radio') return 'radio';
        if (type === 'submit' || type === 'button' || type === 'reset') return 'button';
        return 'textbox';
      }
      if (tag === 'textarea') return 'textbox';
      if (tag === 'select') return 'combobox';
      if (tag === 'summary') return 'button';
      if (/^h[1-6]$/.test(tag)) return 'heading';
      if (tag === 'li') return 'listitem';
      return null;
    };

    const __nameFor = (el) => {
      const aria = __normalize(el.getAttribute('aria-label') || '');
      if (aria) return aria;
      const labelledBy = __normalize(el.getAttribute('aria-labelledby') || '');
      if (labelledBy) {
        const text = labelledBy.split(/\s+/).map((id) => document.getElementById(id)).filter(Boolean).map((node) => __normalize(node.textContent || '')).join(' ').trim();
        if (text) return text;
      }
      if (String(el.tagName || '').toLowerCase() === 'input') {
        const placeholder = __normalize(el.getAttribute('placeholder') || '');
        if (placeholder) return placeholder;
        const value = __normalize(el.value || '');
        if (value) return value;
      }
      const title = __normalize(el.getAttribute('title') || '');
      if (title) return title;
      return __normalize(el.innerText || el.textContent || '').slice(0, 120);
    };

    const __cssPath = (el) => {
      if (!el || el.nodeType !== 1) return null;
      if (el.id) return '#' + CSS.escape(el.id);
      const parts = [];
      let cur = el;
      while (cur && cur.nodeType === 1) {
        let part = String(cur.tagName || '').toLowerCase();
        if (!part) break;
        if (cur.id) {
          part += '#' + CSS.escape(cur.id);
          parts.unshift(part);
          break;
        }
        const parent = cur.parentElement;
        if (parent) {
          const siblings = Array.from(parent.children).filter((node) => String(node.tagName || '').toLowerCase() === part);
          if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(cur) + 1})`;
        }
        parts.unshift(part);
        cur = parent;
        if (parts.length >= 6) break;
      }
      return parts.join(' > ');
    };

    const __root = __scopeSelector
      ? (document.querySelector(__scopeSelector) || document.body || document.documentElement)
      : (document.body || document.documentElement);
    const __entries = [];
    const __seen = new Set();
    const __appendEntry = (el, depth, forcedRole) => {
      if (!__isVisible(el)) return;
      const role = forcedRole || __normalize(el.getAttribute('role') || '').toLowerCase() || __implicitRole(el) || '';
      if (!role || (__interactiveOnly && !__interactiveRoles.has(role))) return;
      if (!__interactiveOnly) {
        if (!__interactiveRoles.has(role) && !__contentRoles.has(role)) return;
        if (__compact && __structuralRoles.has(role) && !__nameFor(el)) return;
      }
      const selector = __cssPath(el);
      if (!selector || __seen.has(selector)) return;
      __seen.add(selector);
      __entries.push({ selector, role, name: __nameFor(el), depth });
    };
    const __walk = (node, depth) => {
      if (!node || depth > __maxDepth || node.nodeType !== 1) return;
      __appendEntry(node, depth, null);
      for (const child of Array.from(node.children || [])) __walk(child, depth + 1);
    };
    if (__root) __walk(__root, 0);
    if (__includeCursor && __root) {
      for (const el of Array.from(__root.querySelectorAll('*'))) {
        if (!__isVisible(el)) continue;
        const style = getComputedStyle(el);
        const tabIndex = el.getAttribute('tabindex');
        if (typeof el.onclick !== 'function' && !el.hasAttribute('onclick') && style.cursor !== 'pointer' && (tabIndex == null || String(tabIndex) === '-1')) continue;
        __appendEntry(el, 0, 'generic');
        if (__entries.length >= 256) break;
      }
    }

    const refs = {};
    const selectorRefs = {};
    const lines = ['- document "' + String(document.title || 'page').replaceAll('"', "'") + '"'];
    let index = 1;
    for (const entry of __entries) {
      const ref = `e${index++}`;
      const name = String(entry.name || '').trim();
      refs[ref] = name ? { role: entry.role, name } : { role: entry.role };
      selectorRefs[ref] = { selector: entry.selector, tag: entry.role, text: name };
      let line = '  '.repeat(Math.max(0, entry.depth)) + '- ' + entry.role;
      if (name) line += ' "' + name.replaceAll('"', "'") + '"';
      line += ` [ref=${ref}]`;
      lines.push(line);
    }
    if (__entries.length === 0) {
      const excerpt = String(document.body?.innerText || '').replace(/[\n\t]/g, ' ').trim().slice(0, 240);
      lines.push(excerpt ? '- text "' + excerpt.replaceAll('"', "'") + '"' : '- (empty)');
    }
    window.__cmuxSnapshotRefs = selectorRefs;
    const title = __normalize(document.title || '');
    const url = String(location.href || '');
    const ready_state = String(document.readyState || '');
    const text = document.body ? String(document.body.innerText || '') : '';
    const html = document.documentElement ? String(document.documentElement.outerHTML || '') : '';
    return { ok: true, value: {
      snapshot: lines.join('\n'), title, url, ready_state,
      page: { title, url, ready_state, text, html }, refs
    } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#;

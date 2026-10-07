// Islands mounted into elements provided by the Dioxus app:
// - the top bar (D-031): Libraries.dev Metal (metal-fx, MIT, Jakub Antalik;
//   its liquidMetal shader is Paper Shaders, Apache-2.0) on the dark pill from
//   the user's reference video, see top-bar.css;
// - the format bar (D-027): the Libraries.dev Gooey playground's DragCard demo
//   (demos/DragCard.tsx, MIT, Copyright (c) 2026 Jakub Antalik), with the
//   PlusMenu demo's surface and shadow (theme.ts), see format-bar.css.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Liquid } from 'liquid-gooey';
import { MetalFx, useMetalBend } from 'metal-fx';
import './gooey-surface.css';
import './format-bar.css';
import './top-bar.css';
import aiIcon from './icons/cyborg.svg';
import analyzeIcon from './icons/person-selecting-note.svg';
import askIcon from './icons/comment-bubble.svg';
import openIcon from './icons/folder-pen.svg';
import newIcon from './icons/clipboard-new.svg';
import saveIcon from './icons/cartoon-floppy-disk.svg';
import pagesIcon from './icons/panel-right-open.svg';
import sunIcon from './icons/cartoon-sun.svg';
import moonIcon from './icons/cartoon-moon.svg';
import gripIcon from './icons/cartoon-grip-vertical.svg';
import minusIcon from './icons/cartoon-minus.svg';
import biggerIcon from './icons/cartoon-plus.svg';
import chevronIcon from './icons/cartoon-chevron-down.svg';
import boldIcon from './icons/bold.svg';
import italicIcon from './icons/italic.svg';
import underlineIcon from './icons/underline.svg';

// Gooey demo defaults (PlusMenu.tsx DEFAULTS), used by the format bar.
const BLUR = 6;
const CONTRAST = 18;

// "Figma soft" shadow (theme.ts), with the demo's display-p3 variants.
const P3 = typeof CSS !== 'undefined' && CSS.supports?.('color', 'color(display-p3 0 0 0 / 0.2)');
const p3 = (tpl) =>
  tpl
    .replace(/\{w4\}/g, P3 ? 'color(display-p3 1 1 1 / 0.04)' : 'rgba(255, 255, 255, 0.04)')
    .replace(/\{w3\}/g, P3 ? 'color(display-p3 1 1 1 / 0.03)' : 'rgba(255, 255, 255, 0.03)')
    .replace(/\{k6\}/g, P3 ? 'color(display-p3 0 0 0 / 0.06)' : 'rgba(0, 0, 0, 0.06)')
    .replace(/\{k5\}/g, P3 ? 'color(display-p3 0 0 0 / 0.05)' : 'rgba(0, 0, 0, 0.05)')
    .replace(/\{k24\}/g, P3 ? 'color(display-p3 0 0 0 / 0.24)' : 'rgba(0, 0, 0, 0.24)');
const SHADOWS = {
  light: '0 0 0 1px rgba(0, 0, 0, 0.06), 0 2px 6px rgba(0, 0, 0, 0.05), 0 4px 42px rgba(0, 0, 0, 0.06)',
  dark: p3(
    '0 0 0 1px {w4} inset, 0 1px 0 0 {w3} inset, ' +
      '0 0 0 1px {k6}, 0 2px 6px 0 {k5}, 0 4px 42px 0 {k24}',
  ),
};

// ---------- theme (D-022) ----------
// The chosen theme lives on <html data-theme> and in localStorage under
// THEME_KEY; without a choice the page follows the system. The app applies a
// saved choice at start-up (crates/pdit-app/src/theme.rs, same key).
const THEME_KEY = 'pdit-theme';
const systemDark = () => window.matchMedia('(prefers-color-scheme: dark)');
const currentTheme = () => {
  const chosen = document.documentElement.dataset.theme;
  if (chosen === 'light' || chosen === 'dark') return chosen;
  return systemDark().matches ? 'dark' : 'light';
};

function chooseTheme(theme) {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Storage can be unavailable (private windows); the choice then lasts for this visit.
  }
}

function useTheme() {
  const [theme, setTheme] = useState(currentTheme);
  useEffect(() => {
    const update = () => setTheme(currentTheme());
    const media = systemDark();
    media.addEventListener('change', update);
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => {
      media.removeEventListener('change', update);
      observer.disconnect();
    };
  }, []);
  return theme;
}

// Koboyo SVGs are inlined so they take `currentColor`; they are static assets
// bundled with the app, not user content.
function Icon({ svg }) {
  return <span aria-hidden="true" style={{ display: 'contents' }} dangerouslySetInnerHTML={{ __html: svg }} />;
}

// ---------- top bar (D-031) ----------

/** One labelled item of the bar: a Koboyo icon and its text (user: every icon
 *  gets a label for now). */
function BarItem({ icon, label, onClick, disabled, pressed }) {
  return (
    <button
      type="button"
      className={pressed ? 'tb-item is-on' : 'tb-item'}
      disabled={disabled}
      aria-pressed={pressed}
      onClick={onClick}
    >
      <span className="tb-icon" aria-hidden="true" dangerouslySetInnerHTML={{ __html: icon }} />
      {label}
    </button>
  );
}

// ---------- the AI menu (D-055, D-060) ----------

/** Where the drops sit: a half circle under the round button (Gooey's plus-menu satellites), for 1 or 2. */
const AI_RADIUS = 96;
const AI_SPOTS = { 1: [90], 2: [124, 56] };
const aiSpot = (n, i) => {
  const a = ((AI_SPOTS[n] || AI_SPOTS[2])[i] * Math.PI) / 180;
  return { x: Math.round(Math.cos(a) * AI_RADIUS), y: Math.round(Math.sin(a) * AI_RADIUS) };
};
const AI_ICONS = { analyze: analyzeIcon, reanalyze: analyzeIcon, ask: askIcon };

/** The AI button's choices, dropping out of it like liquid (liquid-gooey "Morph"): the first time a card that
 *  asks to download the models, afterwards the drops the app sends (`ai.drops`). It sits on the round button, next
 *  to the bar (the bar clips its contents). The app gets the user's picks through the callbacks. */
function AiMenu({ ai, open, setOpen, onAiPick, onAiDownload, onAiCancel }) {
  const theme = useTheme();
  const needs = !!ai?.needsModels;
  const drops = ai?.drops || [];
  const progress = ai?.progress; // [done, total] while downloading
  const asking = open && needs;
  const shown = open && !needs;
  const mb = (n) => Math.round(n / 1e6);
  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => {
      if (progress) return;
      if (!e.target.closest('.tb-ai-menu, .tb-ai')) setOpen(false);
    };
    window.addEventListener('pointerdown', close, true);
    return () => window.removeEventListener('pointerdown', close, true);
  }, [open, progress, setOpen]);
  return (
    <div className={theme === 'dark' ? 'tb-ai-menu pdit-plus-menu is-dark' : 'tb-ai-menu pdit-plus-menu'}>
      <Liquid blur={BLUR} contrast={CONTRAST} fill="var(--modal-bg)" shadow={theme === 'dark' ? SHADOWS.dark : SHADOWS.light}>
        {/* Under the metal button: the liquid the drops and the card flow out of. */}
        <Liquid.Item className="tb-ai-spot" x={0} y={0}>
          <span className="tb-ai-anchor" />
        </Liquid.Item>
        {drops.map((d, i) => {
          const { x, y } = aiSpot(drops.length, i);
          return (
            <Liquid.Item key={d.id} className="tb-ai-spot" x={shown ? x : 0} y={shown ? y : 0} transition="bouncy" delay={i * 45}>
              <button
                type="button"
                className="tb-ai-drop"
                aria-label={d.label}
                title={d.title || d.label}
                disabled={!!d.disabled || !shown}
                style={{ opacity: shown ? 1 : 0 }}
                onClick={() => { setOpen(false); onAiPick?.(d.id); }}
                dangerouslySetInnerHTML={{ __html: AI_ICONS[d.id] || analyzeIcon }}
              />
            </Liquid.Item>
          );
        })}
        <Liquid.Item className="tb-ai-card-spot" x={0} y={0} transition="bouncy" morph={{ shape: true }}>
          <div className={asking ? 'tb-ai-card is-open' : 'tb-ai-card'} role={asking ? 'dialog' : undefined} aria-label="AI models">
            {asking && (
              <>
                <b>pdit's AI needs its models first</b>
                <span>
                  About <b>760 MB</b>, downloaded once to this computer. Only the models — your PDFs never leave your
                  computer.
                </span>
                {progress && (
                  <>
                    <span className="small">
                      {progress[0] >= progress[1] && progress[1] > 0
                        ? 'Getting ready…'
                        : `Downloading… ${mb(progress[0])} of ${mb(progress[1])} MB`}
                    </span>
                    <span className="bar">
                      <i style={{ width: `${progress[1] ? (progress[0] / progress[1]) * 100 : 0}%` }} />
                    </span>
                  </>
                )}
                {ai?.error && <span className="small err">The download stopped: {ai.error}. You can try again.</span>}
                <span className="row">
                  <button type="button" className="btn" onClick={() => (progress ? onAiCancel?.() : setOpen(false))}>
                    {progress ? 'Cancel' : 'Not now'}
                  </button>
                  {!progress && (
                    <button type="button" className="btn primary" onClick={() => onAiDownload?.()}>
                      Download
                    </button>
                  )}
                </span>
              </>
            )}
          </div>
        </Liquid.Item>
      </Liquid>
      {shown && (
        <div className="tb-ai-captions">
          {drops.map((d, i) => {
            const { x, y } = aiSpot(drops.length, i);
            return (
              <span key={d.id} className={d.disabled ? 'tb-ai-cap is-off' : 'tb-ai-cap'} style={{ left: 22 + x, top: 22 + y + 26 }}>
                {d.label}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** The top bar: the round metal button (the AI button, D-055: opens the AI menu),
 *  then Open, New, Pages, Light/Dark and Save. The bar is a dark pill in both
 *  themes, so the metal is pinned to its dark tuning. The bar's own wandering
 *  halo is off (it flashed outside the bar at load, user report); the round
 *  button keeps its glow, clipped to the pill. Its reflections on the items
 *  are off too: they drew a grey block beside "Open". */
function TopBar({ hasDoc, pagesShown, ai, onAi, onAiPick, onAiDownload, onAiCancel, onOpen, onNew, onSave, onTogglePages }) {
  const theme = useTheme();
  const [aiOpen, setAiOpen] = useState(false);
  useEffect(() => { if (!hasDoc) setAiOpen(false); }, [hasDoc]);
  const bar = useRef(null);
  useMetalBend(bar);
  const other = theme === 'dark' ? 'light' : 'dark';
  return (
    <>
    <MetalFx ref={bar} variant="button" preset="chromatic" theme="dark" innerShadow disableGlow>
      <nav className="tb-bar" aria-label="pdit">
        <MetalFx variant="circle" preset="chromatic" theme="dark">
          <button
            type="button"
            className="tb-round tb-ai"
            aria-label="AI"
            title={hasDoc ? 'AI' : 'AI — open a PDF first'}
            aria-haspopup="menu"
            aria-expanded={aiOpen}
            disabled={!hasDoc}
            onClick={() => { setAiOpen((o) => !o); onAi?.(); }}
          >
            <span className="tb-icon" aria-hidden="true" dangerouslySetInnerHTML={{ __html: aiIcon }} />
          </button>
        </MetalFx>
        <div className="tb-items">
          <BarItem icon={openIcon} label="Open" onClick={onOpen} />
          <BarItem icon={newIcon} label="New" onClick={onNew} />
          <BarItem
            icon={pagesIcon}
            label="Pages"
            disabled={!hasDoc}
            pressed={hasDoc && pagesShown}
            onClick={onTogglePages}
          />
          <BarItem
            icon={theme === 'dark' ? moonIcon : sunIcon}
            label={theme === 'dark' ? 'Dark' : 'Light'}
            onClick={() => chooseTheme(other)}
          />
          <BarItem icon={saveIcon} label="Save" disabled={!hasDoc} onClick={onSave} />
        </div>
      </nav>
    </MetalFx>
    {hasDoc && (
      <AiMenu ai={ai} open={aiOpen} setOpen={setAiOpen} onAiPick={onAiPick} onAiDownload={onAiDownload} onAiCancel={onAiCancel} />
    )}
    </>
  );
}

/** Renders the top bar into `element`. The app calls `update({hasDoc,
 *  pagesShown})` when those change; the callbacks report the user's clicks. */
export function mountTopBar(element, callbacks = {}) {
  element.classList.add('pdit-top-bar');
  const root = createRoot(element);
  const render = (state) => root.render(<TopBar {...state} {...callbacks} />);
  render({ hasDoc: false, pagesShown: true, ai: null });
  return { update: render, unmount: () => root.unmount() };
}

// ---------- format bar (D-027) ----------

const FONTS = [
  ['Document', 'PDF font'],
  ['NotoSans', 'Noto Sans'],
];
const COLORS = [
  ['Black', [0, 0, 0]],
  ['Gray', [107, 107, 107]],
  ['Red', [211, 58, 44]],
  ['Blue', [31, 95, 214]],
  ['Green', [31, 138, 76]],
];
const rgb = ([r, g, b]) => `rgb(${r}, ${g}, ${b})`;
const lerp = (a, b, t) => a + (b - a) * t;
// DragCard's Move settings: the surface stays glued to the content; the
// liquid feel is the bow while dragging, no tail.
const DRAG_MOVE = { springiness: 1, stretch: 0, advanced: { tail: 0, bend: 0.6, bendX: 0.35 } };
const BAR_HEIGHT = 46;
const EDGE = 8;
// Where the user dragged the bar, kept for later lines this visit (D-027).
let placedByUser = null;

/** The right-click menu's dropdown (context-menu.css), opening under a button. */
function Dropdown({ label, items, onPick, onClose }) {
  const [shown, setShown] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    const frame = requestAnimationFrame(() => requestAnimationFrame(() => setShown(true)));
    const onDown = (e) => {
      if (!ref.current?.contains(e.target)) onClose();
    };
    window.addEventListener('pointerdown', onDown, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener('pointerdown', onDown, true);
    };
  }, [onClose]);
  return (
    <div ref={ref} className={`cm-menu t-dropdown ${shown ? 'is-open' : ''}`} role="menu">
      <div className="cm-label">{label}</div>
      {items.map(([key, content]) => (
        <button
          key={key}
          type="button"
          role="menuitem"
          onClick={() => {
            onPick(key);
            onClose();
          }}
        >
          {content}
        </button>
      ))}
    </div>
  );
}

/** Above the selected line (its highlight on the page), kept in the window. */
function anchorPosition(width) {
  const line = document.querySelector('.sa-root .sa-highlight');
  if (!line) return null;
  const r = line.getBoundingClientRect();
  return {
    x: Math.max(EDGE, Math.min(r.left, window.innerWidth - width - EDGE)),
    y: Math.max(EDGE, r.top - BAR_HEIGHT - 10),
  };
}

function FormatBar({ visible, style, onChange, inline }) {
  const theme = useTheme();
  const [pos, setPos] = useState(placedByUser);
  const [dragging, setDragging] = useState(false);
  const [lift, setLift] = useState(0);
  const [menu, setMenu] = useState(null);
  const card = useRef(null);
  const drag = useRef(null);

  // Follow the line (scroll, resize, re-render) until the user places the bar.
  // Inline (D-035): the edit panel places the bar, so it neither follows nor drags.
  useLayoutEffect(() => {
    if (!visible || inline || placedByUser) return undefined;
    const follow = () => {
      const next = anchorPosition(card.current?.offsetWidth ?? 0);
      if (next) setPos((p) => (p && p.x === next.x && p.y === next.y ? p : next));
    };
    follow();
    window.addEventListener('scroll', follow, true);
    window.addEventListener('resize', follow);
    return () => {
      window.removeEventListener('scroll', follow, true);
      window.removeEventListener('resize', follow);
    };
  });

  // DragCard's lift: the shadow deepens over 300 ms while dragging.
  useEffect(() => {
    const from = lift;
    const to = dragging ? 1 : 0;
    if (from === to) return undefined;
    let raf = 0;
    const t0 = performance.now();
    const step = (now) => {
      const p = Math.min(1, (now - t0) / 300);
      const e = p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2;
      setLift(from + (to - from) * e);
      if (p < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragging]);

  const closeMenu = useCallback(() => setMenu(null), []);
  if (!visible || !style) return null;

  const onPointerDown = (e) => {
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      /* synthetic events have no active pointer */
    }
    const at = pos ?? { x: 0, y: 0 };
    drag.current = { id: e.pointerId, dx: e.clientX - at.x, dy: e.clientY - at.y };
    setDragging(true);
    setMenu(null);
  };
  const onPointerMove = (e) => {
    const d = drag.current;
    if (!d || e.pointerId !== d.id) return;
    const width = card.current?.offsetWidth ?? 0;
    const next = {
      x: Math.max(EDGE, Math.min(e.clientX - d.dx, window.innerWidth - width - EDGE)),
      y: Math.max(EDGE, Math.min(e.clientY - d.dy, window.innerHeight - BAR_HEIGHT - EDGE)),
    };
    placedByUser = next;
    setPos(next);
  };
  const endDrag = () => {
    drag.current = null;
    setDragging(false);
  };

  const change = (patch) => onChange?.({ ...style, ...patch });
  const size = Math.round(style.size);
  const shadow =
    theme === 'dark'
      ? SHADOWS.dark
      : [
          '0 0 0 1px rgba(0,0,0,0.08)',
          `0 ${lerp(1, 3, lift).toFixed(2)}px ${lerp(3, 5, lift).toFixed(2)}px rgba(0,0,0,0.04)`,
          `0 ${lerp(0, 10, lift).toFixed(2)}px ${lerp(0, 24, lift).toFixed(2)}px rgba(0,0,0,${(0.04 * lift).toFixed(3)})`,
        ].join(', ');
  const at = pos ?? { x: -9999, y: -9999 };
  const dragHandlers = inline
    ? {}
    : { onPointerDown, onPointerMove, onPointerUp: endDrag, onPointerCancel: endDrag };

  return (
    <div
      className={inline ? 'pdit-format-bar is-inline' : 'pdit-format-bar'}
      style={inline ? undefined : { left: at.x, top: at.y }}
      role="toolbar"
      aria-label="Text format"
      // Clicking a control keeps the typing focus in the edit bar.
      onMouseDown={(e) => e.target instanceof Element && e.target.closest('button') && e.preventDefault()}
    >
      <Liquid blur={BLUR} contrast={CONTRAST} fill="var(--modal-bg)" shadow={shadow}>
        <Liquid.Item effect="move" move={DRAG_MOVE}>
          <div className="dgc-card" ref={card} style={{ '--dgc-cb': 0.3 }}>
            <span className="dgc-chip" title={inline ? undefined : 'Drag to move'} {...dragHandlers}>
              <Icon svg={gripIcon} />
            </span>
            <div className="fb-group">
              <button type="button" className="sa-control" aria-haspopup="menu" onClick={() => setMenu(menu === 'font' ? null : 'font')}>
                {FONTS.find(([key]) => key === style.font)?.[1] ?? 'PDF font'} <Icon svg={chevronIcon} />
              </button>
              {menu === 'font' && (
                <Dropdown
                  label="Font"
                  items={FONTS.map(([key, name]) => [key, <span>{name}</span>])}
                  onPick={(font) => change({ font })}
                  onClose={closeMenu}
                />
              )}
              <button type="button" className="sa-control" aria-label="Smaller" onClick={() => change({ size: Math.max(4, size - 1) })}>
                <Icon svg={minusIcon} />
              </button>
              <span className="fb-size" aria-label="Font size">{size}</span>
              <button type="button" className="sa-control" aria-label="Bigger" onClick={() => change({ size: Math.min(144, size + 1) })}>
                <Icon svg={biggerIcon} />
              </button>
            </div>
            <div className="fb-group">
              <button type="button" className="sa-control" aria-label="Colour" aria-haspopup="menu" onClick={() => setMenu(menu === 'color' ? null : 'color')}>
                <span className="fb-swatch" style={{ background: rgb(style.color) }} />
                <Icon svg={chevronIcon} />
              </button>
              {menu === 'color' && (
                <Dropdown
                  label="Colour"
                  items={COLORS.map(([name, color]) => [
                    name,
                    <>
                      <span className="fb-swatch" style={{ background: rgb(color) }} />
                      <span>{name}</span>
                    </>,
                  ])}
                  onPick={(name) => change({ color: COLORS.find(([n]) => n === name)[1] })}
                  onClose={closeMenu}
                />
              )}
              <button type="button" className={`sa-control ${style.bold ? 'is-on' : ''}`} aria-pressed={style.bold} aria-label="Bold" onClick={() => change({ bold: !style.bold })}>
                <Icon svg={boldIcon} />
              </button>
              <button type="button" className={`sa-control ${style.italic ? 'is-on' : ''}`} aria-pressed={style.italic} aria-label="Italic" onClick={() => change({ italic: !style.italic })}>
                <Icon svg={italicIcon} />
              </button>
              <button type="button" className={`sa-control ${style.underline ? 'is-on' : ''}`} aria-pressed={style.underline} aria-label="Underline" onClick={() => change({ underline: !style.underline })}>
                <Icon svg={underlineIcon} />
              </button>
            </div>
          </div>
        </Liquid.Item>
      </Liquid>
    </div>
  );
}

/** Renders the format bar into `element`. The app calls `update({visible,
 *  style})` whenever the selection or its style changes; `onChange(style)`
 *  reports a change made in the bar. `inline` (D-035): the bar sits inside the
 *  app's edit panel instead of floating (position static, no dragging). */
export function mountFormatBar(element, { onChange, inline = false } = {}) {
  element.classList.add('pdit-plus-menu', 'sa-root');
  const root = createRoot(element);
  const render = (state) => root.render(<FormatBar {...state} onChange={onChange} inline={inline} />);
  render({ visible: false, style: null });
  return { update: render, unmount: () => root.unmount() };
}

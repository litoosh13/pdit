// Islands mounted into elements provided by the Dioxus app:
// - the frame (D-062): the centre bar (Undo · Edit | AI · Redo) and the bottom bar (file name, zoom, page count,
//   full screen, Save), laid out like the Canva editor with Devigner UI (devignerui, MIT: Slider, Badge) and
//   Devigner Icons (@devigner-ui/icons: code MIT; artwork Solar CC BY 4.0 + Iconsax), see frame.css;
// - the format bar (D-027): the Libraries.dev Gooey playground's DragCard demo
//   (demos/DragCard.tsx, MIT, Copyright (c) 2026 Jakub Antalik), with the
//   PlusMenu demo's surface and shadow (theme.ts), see format-bar.css.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Liquid } from 'liquid-gooey';
import { Slider, Badge, DeleteButton, MenuDock } from 'devignerui';
import { IconRedo } from '../node_modules/@devigner-ui/icons/dist/icons/Redo.js';
import { IconEdit } from '../node_modules/@devigner-ui/icons/dist/icons/Edit.js';
import { IconMagicStar } from '../node_modules/@devigner-ui/icons/dist/icons/MagicStar.js';
import { IconDownload } from '../node_modules/@devigner-ui/icons/dist/icons/Download.js';
import { IconMaximize } from '../node_modules/@devigner-ui/icons/dist/icons/Maximize.js';
import { IconLock } from '../node_modules/@devigner-ui/icons/dist/icons/Lock.js';
import { IconLockUnlocked } from '../node_modules/@devigner-ui/icons/dist/icons/LockUnlocked.js';
import { IconCopy } from '../node_modules/@devigner-ui/icons/dist/icons/Copy.js';
import { IconAddSquare } from '../node_modules/@devigner-ui/icons/dist/icons/AddSquare.js';
import { IconPen } from '../node_modules/@devigner-ui/icons/dist/icons/Pen.js';
import { IconChatRound } from '../node_modules/@devigner-ui/icons/dist/icons/ChatRound.js';
import { IconTrashBinMinimalistic } from '../node_modules/@devigner-ui/icons/dist/icons/TrashBinMinimalistic.js';
import { IconMenuDots } from '../node_modules/@devigner-ui/icons/dist/icons/MenuDots.js';
import { IconLink } from '../node_modules/@devigner-ui/icons/dist/icons/Link.js';
import 'devignerui/styles.css';
import './gooey-surface.css';
import './format-bar.css';
import './frame.css';
import gripIcon from './icons/devigner/More.svg';
import minusIcon from './icons/devigner/Minus.svg';
import biggerIcon from './icons/devigner/Plus.svg';
import chevronIcon from './icons/devigner/ChevronDown.svg';
import boldIcon from './icons/devigner/TextBold.svg';
import italicIcon from './icons/devigner/TextItalic.svg';
import underlineIcon from './icons/devigner/TextUnderline.svg';

// Gooey demo defaults (PlusMenu.tsx DEFAULTS), used by the format bar.
const BLUR = 6;
const CONTRAST = 18;

// The format bar's Devigner SVGs (src/icons/devigner, scripts/export-icons.mjs) are inlined so they take `currentColor`; they are static assets bundled with the
// app, not user content.
function Icon({ svg }) {
  return <span aria-hidden="true" style={{ display: 'contents' }} dangerouslySetInnerHTML={{ __html: svg }} />;
}

// ---------- frame (D-062) ----------

/** The page nearest the top of the window (1-based), read from the page list as it scrolls. */
function useCurrentPage(count) {
  const [page, setPage] = useState(1);
  useEffect(() => {
    const update = () => {
      let current = 1;
      for (const el of document.querySelectorAll('.page-list .page[data-page]')) {
        if (el.getBoundingClientRect().top > window.innerHeight / 3) break;
        current = Number(el.dataset.page) + 1;
      }
      setPage(current);
    };
    update();
    window.addEventListener('scroll', update, { passive: true });
    return () => window.removeEventListener('scroll', update);
  }, [count]);
  return Math.min(page, Math.max(count, 1));
}

function Frame({ hasDoc, mode, fileName, zoom, pages, canUndo, onMode, onUndo, onZoom, onSave, onPages }) {
  const current = useCurrentPage(pages);
  const ai = mode === 'ai';
  const fullscreen = () => {
    if (document.fullscreenElement) document.exitFullscreen?.();
    else document.documentElement.requestFullscreen?.();
  };
  if (!hasDoc) return null;
  return (
    <>
      <div className="fr-center" role="toolbar" aria-label="Mode">
        <button type="button" className="fr-icon" aria-label="Undo" title="Undo" disabled={!canUndo} onClick={onUndo}>
          <IconRedo className="fr-mirror" />
        </button>
        <span className="fr-sep" />
        <div role="tablist" className="fr-tabs">
          <button type="button" role="tab" aria-selected={!ai} className={ai ? '' : 'is-on'} onClick={() => onMode?.('edit')}>
            <IconEdit />
            Edit
          </button>
          <button type="button" role="tab" aria-selected={ai} className={ai ? 'is-on' : ''} onClick={() => onMode?.('ai')}>
            <IconMagicStar />
            AI
          </button>
        </div>
        <span className="fr-sep" />
        {/* No redo history yet (LATER: an app-wide undo history). */}
        <button type="button" className="fr-icon" aria-label="Redo" title="Redo — not available yet" disabled>
          <IconRedo />
        </button>
      </div>
      <footer className="fr-bottom">
        <span className="fr-name" title={fileName}>{fileName}</span>
        <div className="fr-right">
          <Slider
            size="sm"
            className="fr-zoom"
            min={25}
            max={400}
            value={zoom}
            onValueChange={(v) => onZoom?.(Math.round(v))}
            format={(v) => `${Math.round(v)}%`}
            aria-label="Zoom"
          />
          {!ai && (
            <Badge size="sm" onClick={onPages} title="Show the pages">
              {current} / {pages}
            </Badge>
          )}
          <button type="button" className="fr-icon" aria-label="Full screen" title="Full screen" onClick={fullscreen}>
            <IconMaximize />
          </button>
          <button type="button" className="fr-save" onClick={onSave}>
            <IconDownload />
            Save
          </button>
        </div>
      </footer>
    </>
  );
}

/** Renders the frame into `element`. The app calls `update(state)` when its state changes; the callbacks report
 *  the user's choices (`onMode('edit' | 'ai')`, `onZoom(percent)`, `onUndo`, `onSave`, `onPages`). */
export function mountFrame(element, callbacks = {}) {
  const root = createRoot(element);
  const render = (state) => root.render(<Frame {...state} {...callbacks} />);
  render({ hasDoc: false });
  return { update: render, unmount: () => root.unmount() };
}

// ---------- page tools (D-062) ----------

/** The row above a page (Canva's page tools): its number, lock, duplicate, add a page after it, and Devigner's
 *  DeleteButton (asks before it deletes). The last page can't be deleted. */
function PageToolsRow({ page, count, locked, onLock, onDuplicate, onAdd, onDelete }) {
  return (
    <div className="pt-row">
      <span className="pt-label">
        Page {page + 1}
        {locked && <span className="pt-locked"> · locked</span>}
      </span>
      <button
        type="button"
        className="fr-icon"
        aria-label={locked ? 'Unlock page' : 'Lock page'}
        aria-pressed={locked}
        title={locked ? 'Unlock: allow changes to this page' : 'Lock: no changes to this page'}
        onClick={onLock}
      >
        {locked ? <IconLock /> : <IconLockUnlocked />}
      </button>
      <button type="button" className="fr-icon" aria-label="Duplicate page" title="Duplicate page" disabled={locked} onClick={onDuplicate}>
        <IconCopy />
      </button>
      <button type="button" className="fr-icon" aria-label="Add a page after this one" title="Add a page after this one" onClick={onAdd}>
        <IconAddSquare />
      </button>
      {count > 1 && !locked && <DeleteButton label="Delete page" confirmLabel="Delete" onConfirm={() => onDelete?.()} resetAfter={0} />}
    </div>
  );
}

/** Renders a page's tools row into `element`; the app calls `update({ page, count, locked })`. */
export function mountPageTools(element, callbacks = {}) {
  const root = createRoot(element);
  const render = (state) => root.render(<PageToolsRow {...state} {...callbacks} />);
  return { update: render, unmount: () => root.unmount() };
}

// ---------- selection dock (D-062) ----------

/** Devigner's MenuDock for the selected text (the approved mockup's bar at the selection): Edit (typing starts in
 *  the text on the page), Comment, Copy, Delete, More ▸ Add link. New text is typed on the page; no dock. */
function SelectionDock({ selKey, adding, text, onEdit, onComment, onDelete, onLink }) {
  const [path, setPath] = useState([]);
  useEffect(() => setPath([]), [selKey]);
  if (!selKey || adding) return null;
  const items = [
    { label: 'Edit', icon: <IconPen />, onSelect: onEdit },
    { label: 'Comment', icon: <IconChatRound />, onSelect: onComment },
    { label: 'Copy', icon: <IconCopy />, onSelect: () => navigator.clipboard?.writeText(text ?? '') },
    { label: 'Delete', icon: <IconTrashBinMinimalistic />, onSelect: onDelete },
    { label: 'More', icon: <IconMenuDots />, items: [{ label: 'Add link', icon: <IconLink />, onSelect: onLink }] },
  ];
  return <MenuDock label="Selected text" items={items} path={path} onNavigate={setPath} />;
}

/** Renders the selection dock into `element`; the app calls `update(state)` as the selection changes. */
export function mountSelectionDock(element, callbacks = {}) {
  const root = createRoot(element);
  const render = (state) => root.render(<SelectionDock {...state} {...callbacks} />);
  render({});
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
  const shadow = [
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

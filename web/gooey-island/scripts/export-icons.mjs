// Writes the Devigner Icons (@devigner-ui/icons, code MIT; artwork Solar CC BY 4.0 + Iconsax, see SOURCES.md)
// that the Dioxus side and the format bar use as plain SVG files, rendered from the package's own components, unmodified.
// Usage: node scripts/export-icons.mjs  (from web/gooey-island). Add names to ICONS when the app needs more.
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { mkdirSync, writeFileSync } from 'node:fs';

const APP = '../../crates/pdit-app/assets/icons/devigner';
const ISLAND = 'src/icons/devigner';
// [out dir, name] (Outline variant)
const ICONS = [
  [APP, 'Folder'],
  [APP, 'File'],
  [APP, 'Printer'],
  [APP, 'ArrowLeft'],
  [APP, 'ArrowUp'],
  [APP, 'Eye'],
  [APP, 'Text'],
  [APP, 'GalleryAdd'],
  [APP, 'Shapes'],
  [APP, 'DocumentText'],
  [APP, 'DocumentNormal'],
  [APP, 'Crown'],
  [APP, 'Copy'],
  [APP, 'Import'],
  [APP, 'Export'],
  [APP, 'RotateLeft'],
  [APP, 'TrashBinMinimalistic'],
  [APP, 'UndoLeft'],
  [APP, 'SortVertical'],
  [APP, 'Pen2'],
  [APP, 'Cursor'],
  [APP, 'Magicpen'],
  [APP, 'Link'],
  [APP, 'ArrowRightUp'],
  [APP, 'Stop'],
  [APP, 'Stop2'],
  [APP, 'Minus'],
  [APP, 'Grid3x3'],
  [APP, 'Plus'],
  [APP, 'Brush2'],
  [APP, 'TextUnderline'],
  [APP, 'TextCross'],
  [APP, 'MessageText'],
  [APP, 'Pen'],
  [APP, 'Sticker'],
  [APP, 'Messages'],
  [APP, 'LayoutHeader'],
  [APP, 'Waterdrop'],
  [APP, 'Bookmark'],
  [APP, 'ClipboardList'],
  [APP, 'TextField'],
  [APP, 'CheckSquare'],
  [APP, 'RecordCircle'],
  [APP, 'ChevronDownSquare'],
  [APP, 'Settings'],
  [APP, 'SearchNormal'],
  [APP, 'Close'],
  [APP, 'ChevronUp'],
  [APP, 'ChevronDown'],
  [ISLAND, 'More'],
  [ISLAND, 'Minus'],
  [ISLAND, 'Plus'],
  [ISLAND, 'ChevronDown'],
  [ISLAND, 'TextBold'],
  [ISLAND, 'TextItalic'],
  [ISLAND, 'TextUnderline']
];

for (const [out, name] of ICONS) {
  const mod = await import(`@devigner-ui/icons/${name}`);
  mkdirSync(out, { recursive: true });
  writeFileSync(`${out}/${name}.svg`, renderToStaticMarkup(createElement(mod.default, { variant: 'Outline' })) + '\n');
  console.log(out, name);
}

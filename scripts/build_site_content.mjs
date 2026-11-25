#!/usr/bin/env node
/**
 * Prepare generated Astro content from the repository's canonical documentation.
 * Uses Node's standard library so local builds work on Windows, Linux, and macOS.
 */
import { copyFile, cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const site = path.join(root, 'site');
const docsSource = path.join(root, 'docs');
const docsTarget = path.join(site, 'src/content/docs/docs');
const dataTarget = path.join(site, 'src/data/diagnostics.json');
const publicAssets = path.join(site, 'public/assets');
const sourceAssets = path.join(site, 'src/assets');
const indexSource = path.join(site, 'content/docs-index.mdx');

const codePattern = /^`((?:LEX|PAR|AST|PRE|SEM)\d{3})`$/;
const sourcePattern = /"((?:LEX|PAR|SEM|PRE|AST)\d{3})"/g;

function cleanMarkdown(value) {
  return value
    .replace(/\*\*(.*?)\*\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\[(.*?)\]\((.*?)\)/g, '$1')
    .replace(/\s+/g, ' ')
    .trim();
}

function tableCells(line) {
  return line.trim().replace(/^\||\|$/g, '').split('|').map((cell) => cell.trim());
}

async function collectRustFiles(directory) {
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch {
    return [];
  }
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await collectRustFiles(fullPath));
    else if (entry.isFile() && entry.name.endsWith('.rs')) files.push(fullPath);
  }
  return files;
}

async function parseDiagnostics() {
  const source = await readFile(path.join(docsSource, 'diagnostics.md'), 'utf8');
  let section = '';
  const rules = new Map();

  for (const rawLine of source.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === '## Lexical diagnostics') section = 'Lexical';
    else if (line === '## Parser/syntax diagnostics' || line === '### AST parser diagnostics') section = 'Syntax';
    else if (line === '## Preprocessor diagnostics') section = 'Preprocessor';
    else if (line === '## Semantic and lint diagnostics') section = 'Semantic';

    if (!line.startsWith('|')) continue;
    const cells = tableCells(line);
    if (cells.length < 3) continue;
    const match = cells[0].match(codePattern);
    if (!match) continue;

    const code = match[1];
    const summary = cleanMarkdown(cells[1]);
    const detail = cleanMarkdown(cells.length >= 4 ? cells[3] : cells[2]);
    let severity = 'error';
    let kind = section === 'Syntax' ? 'syntax' : 'lexical';
    let family = section;

    if (section === 'Semantic' && cells.length >= 4) {
      const severityText = cleanMarkdown(cells[2]).toLowerCase();
      severity = severityText.includes('warning') ? 'warning' : 'error';
      kind = severityText.includes('lint') ? 'lint' : 'semantic';
      family = kind === 'lint' ? 'Lint' : 'Semantic';
    } else if (section === 'Preprocessor') {
      severity = code === 'PRE037' ? 'info' : 'error';
      kind = 'syntax';
    }

    const numericCode = Number(code.slice(3));
    const astOnly = code.startsWith('AST') || (code.startsWith('SEM') && numericCode >= 24);
    rules.set(code, {
      code,
      summary,
      detail,
      category: code.slice(0, 3),
      family,
      kind,
      severity,
      engine: astOnly ? 'AST only' : 'AST + Legacy',
    });
  }

  if (rules.size === 0) throw new Error('No diagnostics were parsed from docs/diagnostics.md');

  const sourceFiles = [
    path.join(root, 'src/lexer.rs'),
    path.join(root, 'src/parser.rs'),
    path.join(root, 'src/semantic.rs'),
    ...await collectRustFiles(path.join(root, 'src/preprocessor')),
    ...await collectRustFiles(path.join(root, 'src/ast')),
    ...await collectRustFiles(path.join(root, 'src/ast_semantic')),
  ];
  const sourceCodes = new Set();
  for (const file of sourceFiles) {
    let content;
    try {
      content = await readFile(file, 'utf8');
    } catch {
      continue;
    }
    for (const match of content.matchAll(sourcePattern)) sourceCodes.add(match[1]);
  }

  const missing = [...sourceCodes].filter((code) => !rules.has(code)).sort();
  if (missing.length) throw new Error(`Diagnostic catalog is missing source codes: ${missing.join(', ')}`);

  return [...rules.values()].sort((a, b) => a.code.localeCompare(b.code));
}

async function addMissingFrontmatter(directory, rootDirectory = directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      await addMissingFrontmatter(fullPath, rootDirectory);
      continue;
    }
    if (!entry.isFile() || !entry.name.endsWith('.md')) continue;

    const content = await readFile(fullPath, 'utf8');
    const relativePath = path.relative(rootDirectory, fullPath).split(path.sep).join('/');
    const editUrl = 'https://github.com/mindwork64/neolint/edit/main/docs/' + relativePath;
    const heading = content.match(/^#\s+(.+?)\s*#*\s*$/m);
    const fallback = path.basename(entry.name, '.md')
      .replace(/[-_]+/g, ' ')
      .replace(/\b\w/g, (character) => character.toUpperCase());
    const title = heading?.[1]?.trim() || fallback;
    const frontmatterMatch = content.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?/);

    if (frontmatterMatch) {
      const lines = frontmatterMatch[1].split(/\r?\n/);
      if (!lines.some((line) => /^title:\s*/.test(line))) {
        lines.push('title: ' + JSON.stringify(title));
      }
      const editLine = 'editUrl: ' + JSON.stringify(editUrl);
      const editIndex = lines.findIndex((line) => /^editUrl:\s*/.test(line));
      if (editIndex >= 0) lines[editIndex] = editLine;
      else lines.push(editLine);

      const updated = '---\n' + lines.join('\n') + '\n---\n' + content.slice(frontmatterMatch[0].length);
      await writeFile(fullPath, updated, 'utf8');
    } else {
      const frontmatter = '---\ntitle: ' + JSON.stringify(title) + '\neditUrl: ' + JSON.stringify(editUrl) + '\n---\n\n';
      await writeFile(fullPath, frontmatter + content, 'utf8');
    }
  }
}

async function main() {
  await rm(docsTarget, { recursive: true, force: true });
  await cp(docsSource, docsTarget, { recursive: true });
  await mkdir(docsTarget, { recursive: true });
  await addMissingFrontmatter(docsTarget);
  await copyFile(indexSource, path.join(docsTarget, 'index.mdx'));

  await mkdir(publicAssets, { recursive: true });
  await mkdir(sourceAssets, { recursive: true });
  await copyFile(path.join(root, 'assets/branding/neolint-logo.png'), path.join(publicAssets, 'neolint-logo.png'));
  await copyFile(path.join(root, 'assets/branding/neolint-logo.png'), path.join(sourceAssets, 'neolint-logo.png'));
  await copyFile(path.join(root, 'assets/branding/neolint-icon.png'), path.join(publicAssets, 'neolint-icon.png'));
  await copyFile(path.join(root, 'assets/branding/neolint.exe.ico'), path.join(site, 'public/favicon.ico'));

  const diagnostics = await parseDiagnostics();
  await mkdir(path.dirname(dataTarget), { recursive: true });
  await writeFile(dataTarget, `${JSON.stringify(diagnostics, null, 2)}\n`, 'utf8');

  const docsFiles = [];
  async function walk(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) await walk(fullPath);
      else if (entry.name.endsWith('.md')) docsFiles.push(fullPath);
    }
  }
  await walk(docsTarget);
  console.log(`Prepared ${docsFiles.length} documentation files`);
  console.log(`Prepared ${diagnostics.length} diagnostic entries`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});

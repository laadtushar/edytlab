import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {test} from 'node:test';
import {PROJECT_ROOT, projectPath} from './paths.ts';

await test('promo paths contain new files and reject traversal and symlink escapes', () => {
  const inside = mkdtempSync(join(PROJECT_ROOT, '.paths-test-'));
  const outside = mkdtempSync(join(tmpdir(), 'promo-paths-'));
  const refuses = (path: string) => assert.throws(() => projectPath(path), (error: Error) =>
    error.message.includes(PROJECT_ROOT) && error.message.includes(JSON.stringify(path)));
  try {
    const source = join(inside, 'source.wav');
    writeFileSync(source, 'fixture');
    assert.equal(projectPath(source), source);
    assert.equal(projectPath(join(inside, 'new', 'out.wav')), join(inside, 'new', 'out.wav'));
    assert.equal(projectPath(PROJECT_ROOT), PROJECT_ROOT);
    refuses(join(outside, 'new.wav'));
    refuses(resolve(PROJECT_ROOT, '..', 'escape.wav'));
    symlinkSync(outside, join(inside, 'escaped-dir'));
    refuses(join(inside, 'escaped-dir', 'new.wav'));
    symlinkSync(join(outside, 'missing.wav'), join(inside, 'dangling.wav'));
    refuses(join(inside, 'dangling.wav'));
    symlinkSync(join(outside, 'missing-dir'), join(inside, 'dangling-dir'));
    refuses(join(inside, 'dangling-dir', 'new.wav'));
    mkdirSync(join(inside, 'work'));
    symlinkSync(join(inside, 'work'), join(inside, 'safe-dir'));
    assert.equal(projectPath(join(inside, 'safe-dir', 'new.wav')), join(inside, 'work', 'new.wav'));
  } finally {
    rmSync(inside, {recursive: true, force: true});
    rmSync(outside, {recursive: true, force: true});
  }
});

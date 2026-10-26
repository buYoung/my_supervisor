#!/usr/bin/env node
'use strict';

// Run: node examples/hello-world.js
// Disable automatic restart when registering this example in my-supervisor.
const { spawn } = require('node:child_process');

function main() {
  const args = process.argv.slice(2);
  if (args.includes('--help') || args.includes('-h')) {
    console.log(`Usage: node examples/hello-world.js

Display a native macOS Hello World dialog. Closing it ends the process.

Options:
  -h, --help     Show this help without opening a window
  --version      Show the example version`);
    return;
  }
  if (args.length === 1 && args[0] === '--version') {
    console.log('hello-world 1.0.0');
    return;
  }
  if (args.length > 0 || process.platform !== 'darwin') {
    console.error(args.length > 0 ? 'Unknown argument. Use --help.' : 'This example requires macOS.');
    process.exitCode = 1;
    return;
  }

  const dialog = spawn('/usr/bin/osascript', ['-l', 'JavaScript', '-e', `
    var app = Application.currentApplication();
    app.includeStandardAdditions = true;
    app.displayDialog('Hello World', {
      withTitle: 'my-supervisor 테스트',
      buttons: ['종료'],
      defaultButton: '종료'
    });
  `], { stdio: ['ignore', 'ignore', 'inherit'] });

  let isStopping = false;
  let hasSpawnError = false;
  let killTimer;
  function stop(signal) {
    if (isStopping) return;
    isStopping = true;
    dialog.kill(signal);
    killTimer = setTimeout(() => dialog.kill('SIGKILL'), 1500);
    killTimer.unref();
  }
  const onInterrupt = () => stop('SIGINT');
  const onTerminate = () => stop('SIGTERM');
  process.once('SIGINT', onInterrupt);
  process.once('SIGTERM', onTerminate);

  dialog.once('spawn', () => {
    console.log(`[info] Hello World 창을 열었습니다. PID=${process.pid}`);
  });
  dialog.once('error', error => {
    hasSpawnError = true;
    console.error(`[error] 창을 열지 못했습니다: ${error.message}`);
    process.exitCode = 1;
  });
  dialog.once('close', (code, signal) => {
    clearTimeout(killTimer);
    process.removeListener('SIGINT', onInterrupt);
    process.removeListener('SIGTERM', onTerminate);
    if (hasSpawnError) return;
    process.exitCode = isStopping ? 0 : (code ?? 1);
    if (process.exitCode === 0) {
      console.log('[info] 창이 닫혀 테스트 프로세스를 종료합니다.');
    } else {
      console.error(`[error] 대화상자가 비정상 종료되었습니다: ${signal || code}`);
    }
  });
}

main();

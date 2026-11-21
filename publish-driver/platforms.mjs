#!/usr/bin/env node
// lycodl-platforms-version: 1
//
// Cross-platform constants shared by the publish driver and the capability probe.
// Everything platform-specific (data dir, kuake helper, destination order) lives
// here so Windows/macOS/Linux builds behave the same.
'use strict';

import { homedir } from 'node:os';
import { join } from 'node:path';

export const DESTS = ['quark', 'bilibili', 'douyin', 'xhs', 'kuaishou', 'youtube', 'x'];

export const BROWSER_DESTS = ['bilibili', 'douyin', 'xhs', 'kuaishou', 'youtube', 'x'];

export const COOKIE_RULES = {
  bilibili: [['SESSDATA', '.bilibili.com'], ['DedeUserID', '.bilibili.com']],
  douyin: [['sessionid', '.douyin.com'], ['sid_tt', '.douyin.com']],
  xhs: [['web_session', '.xiaohongshu.com']],
  kuaishou: [['userId', '.kuaishou.com'], ['passToken', 'id.kuaishou.com']],
  youtube: [['LOGIN_INFO', '.youtube.com'], ['SID', '.youtube.com']],
  x: [['auth_token', '.x.com'], ['ct0', '.x.com']],
};

export function dataDir(env = process.env) {
  if (env.LYCODL_DATA_DIR) return env.LYCODL_DATA_DIR;
  if (process.platform === 'win32') {
    return join(env.APPDATA || join(homedir(), 'AppData', 'Roaming'), 'lycodl');
  }
  if (env.XDG_DATA_HOME) return join(env.XDG_DATA_HOME, 'lycodl');
  return join(homedir(), '.local', 'share', 'lycodl');
}

export function defaultQuakeCommand(env = process.env) {
  if (env.LYCODL_QUAKE_COMMAND) return env.LYCODL_QUAKE_COMMAND;
  if (process.platform === 'win32') return '';
  return join(homedir(), '.config', 'kuake', 'run.sh');
}

export function isBrowserDest(dest) {
  return BROWSER_DESTS.indexOf(String(dest || '')) >= 0;
}
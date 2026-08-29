// Telegram 命令机器人（轮询版）——跑在 GitHub Actions cron 上，无需任何服务器。
//
// 工作方式：每次运行调用 getUpdates 拉取「自上次确认以来」的待处理更新，逐条处理命令并回复，
// 最后用 offset=最后一条+1 再调一次 getUpdates 向 Telegram「确认」（这些更新随即被服务端清掉，
// 下次不再返回）。Telegram 自己保存 offset 状态（未确认的更新保留 24h），因此本脚本无需任何持久化存储。
//
// 支持私聊与群组（群组里以 /命令 形式发送即可，命令不受 bot 隐私模式影响）。
// 命令：/help /releases /release <tag> /issues /issue <编号>
//
// 局限：受 GitHub cron 最小 5 分钟间隔限制，命令有延迟（非实时）。要实时需改用 webhook（需 serverless 端点）。
//
// ── 入群验证（反广告）────────────────────────────────────────────────
// 群设为「新成员需批准」后，用户点加入 → 进入 pending：看不到群、发不了消息，
// 直到机器人批准。于是【轮询延迟不再影响安全性】——没验证的人根本进不来，
// 也就不需要"进群后删广告 / 禁言 / 移除"那一整套事后补救。
//
// 流程：chat_join_request → 私聊出一道加法题（选择题按钮）→ 答对 approve、连错 3 次 decline。
// 无状态：正确答案与已答错次数全部编码进按钮的 callback_data，不需要任何持久化存储。
//
// ⚠️ 已知限制（官方文档原文核对过，别再想当然）：
//   ChatJoinRequest.user_chat_id —— "The bot can use this identifier for 5 minutes to send
//   messages until the join request is processed"，即【只有 5 分钟】能私聊该用户。
//   （更新本身在服务器保留 24h，那是另一回事，别混淆——我就混过一次。）
//   而 GitHub cron 最小 5 分钟且常再延后 10~15 分钟 → 轮询模式下验证码【大概率发不出去】。
//
//   安全性不受影响：发不出去时用户仍卡在待批准，进不了群也发不了消息，只是退化成人工审批。
//   故私聊失败时【绝不自动拒绝】，留给管理员人工处理，避免误伤真人。
//
//   要让验证码真正送达，必须让机器人近实时地收到更新，两条路：
//     a) 改 webhook（Cloudflare Workers / Deno Deploy 等，免费且常驻，本文件逻辑可直接复用）
//     b) 自托管长轮询（getUpdates timeout=50 常驻进程，NAS 上跑个小容器即可，无需公网端点）
//
// 需要：机器人是群管理员且有 can_invite_users 权限（否则收不到 chat_join_request）。

const TG = process.env.TG_TOKEN;
const GH = process.env.GH_TOKEN || '';
const REPO = process.env.REPO; // owner/repo
if (!TG) {
  console.log('TELEGRAM_BOT_TOKEN 未配置，跳过');
  process.exit(0);
}

const tgUrl = (method) => `https://api.telegram.org/bot${TG}/${method}`;
const ghHeaders = {
  accept: 'application/vnd.github+json',
  'user-agent': 'woc-telegram-bot',
  ...(GH ? { authorization: `Bearer ${GH}` } : {}),
};

async function tg(method, params) {
  const r = await fetch(tgUrl(method), {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(params),
  });
  return r.json();
}
async function gh(path) {
  const r = await fetch(`https://api.github.com/repos/${REPO}${path}`, { headers: ghHeaders });
  if (!r.ok) throw new Error(`GitHub ${path} → ${r.status}`);
  return r.json();
}
const trunc = (s, n) => (s && s.length > n ? s.slice(0, n) + '…' : s || '');
const send = (chatId, text) => tg('sendMessage', { chat_id: chatId, text, disable_web_page_preview: true });

const HELP = [
  '🤖 云微 WechatOnCloud 机器人命令：',
  '',
  '/releases — 最近发布列表',
  '/release <tag> — 某版本详情（省略 = 最新）',
  '/issues — 打开中的 issue 列表',
  '/issue <编号> — issue 详情',
  '/help — 显示本帮助',
  '',
  '（轮询版，命令可能有几分钟延迟）',
].join('\n');

async function handle(cmd, arg, chatId) {
  switch (cmd) {
    case '/start':
    case '/help':
      return send(chatId, HELP);

    case '/releases': {
      const rels = await gh('/releases?per_page=8');
      if (!rels.length) return send(chatId, '暂无 release');
      const lines = rels.map(
        (r) =>
          `• ${r.tag_name}${r.name && r.name !== r.tag_name ? ' — ' + r.name : ''}  (${(r.published_at || '').slice(0, 10)})`,
      );
      return send(chatId, '📦 最近发布：\n' + lines.join('\n') + '\n\n用 /release <tag> 看某版详情');
    }

    case '/release': {
      const rel = arg ? await gh(`/releases/tags/${encodeURIComponent(arg)}`) : await gh('/releases/latest');
      const title = `${rel.tag_name}${rel.name && rel.name !== rel.tag_name ? ' · ' + rel.name : ''}`;
      return send(
        chatId,
        `📦 ${title}\n发布于 ${(rel.published_at || '').slice(0, 10)}\n\n${trunc(rel.body, 2500)}\n\n🔗 ${rel.html_url}`,
      );
    }

    case '/issues': {
      const items = (await gh('/issues?state=open&per_page=10&sort=updated')).filter((i) => !i.pull_request);
      if (!items.length) return send(chatId, '🎉 当前没有打开的 issue');
      const lines = items.map((i) => `• #${i.number} ${trunc(i.title, 60)}`);
      return send(chatId, `🐛 打开中的 issue（${items.length}）：\n` + lines.join('\n') + '\n\n用 /issue <编号> 看详情');
    }

    case '/issue': {
      if (!arg) return send(chatId, '用法：/issue <编号>');
      const i = await gh(`/issues/${encodeURIComponent(arg)}`);
      if (i.pull_request) return send(chatId, `#${arg} 是个 PR，不是 issue`);
      return send(
        chatId,
        `🐛 #${i.number} ${i.title}\n状态：${i.state} · by ${i.user?.login}\n\n${trunc(i.body, 2500)}\n\n🔗 ${i.html_url}`,
      );
    }

    default:
      return; // 未知命令静默忽略，避免群里刷屏
  }
}

// ---------- 入群验证 ----------
const MAX_TRIES = 3;

// 出题：a + b，答案与干扰项都是两位数量级，干扰项与答案差 1~9 且互不相同。
// 用文字题而非图片验证码：真正的门槛是"需要一次人工交互"，纯文字已能挡掉绝大多数自动化广告号，
// 且不必引图形依赖。若日后出现能自动答题的号，再考虑加干扰图片。
function makeCaptcha() {
  const a = 1 + Math.floor(Math.random() * 20);
  const b = 10 + Math.floor(Math.random() * 90);
  const ans = a + b;
  const opts = new Set([ans]);
  while (opts.size < 4) {
    const d = 1 + Math.floor(Math.random() * 9);
    opts.add(ans + (Math.random() < 0.5 ? -d : d));
  }
  const list = [...opts].sort(() => Math.random() - 0.5);
  return { q: `${a} + ${b} = ?`, ans, list };
}

// callback_data 上限 64 字节：v:<chatId>:<userId>:<ok|no>:<已错次数> ≈ 35 字节，安全。
const cbData = (chatId, userId, ok, tries) => `v:${chatId}:${userId}:${ok ? 'ok' : 'no'}:${tries}`;

function captchaKeyboard(chatId, userId, tries) {
  const c = makeCaptcha();
  return {
    q: c.q,
    markup: {
      inline_keyboard: [
        c.list.map((n) => ({ text: String(n), callback_data: cbData(chatId, userId, n === c.ans, tries) })),
      ],
    },
  };
}

const captchaText = (q, tries) =>
  [
    '👋 欢迎申请加入「云微 WechatOnCloud」群组。',
    '',
    '为了拦截广告号，请先做一道题（点下面的按钮作答）：',
    '',
    `❓ ${q}`,
    '',
    tries > 0 ? `（已答错 ${tries}/${MAX_TRIES} 次，答错 ${MAX_TRIES} 次将拒绝本次申请，可稍后重新申请）` : '（答对后自动放行）',
  ].join('\n');

async function onJoinRequest(r) {
  const chatId = r.chat?.id;
  const userId = r.from?.id;
  const dm = r.user_chat_id; // 官方文档：可在 24h 内用它私聊该用户
  if (!chatId || !userId || !dm) return;
  const { q, markup } = captchaKeyboard(chatId, userId, 0);
  const res = await tg('sendMessage', { chat_id: dm, text: captchaText(q, 0), reply_markup: markup });
  if (!res.ok) {
    // 最常见原因：距离入群申请已超过 5 分钟的私聊窗口（轮询模式下这是常态，不是偶发）。
    // 也可能是用户屏蔽了机器人。一律【不自动拒绝】，留给管理员人工处理，避免误伤真人。
    console.log(
      `⚠️ 验证码私聊失败 user=${userId}: ${res.description}\n` +
        `   多半是超过了 user_chat_id 的 5 分钟窗口（cron 延迟所致）。该用户仍处于待批准状态，` +
        `进不了群，需要管理员在 Telegram 里手动批准/拒绝。`,
    );
  }
}

async function onCallback(cb) {
  const parts = String(cb.data || '').split(':');
  if (parts[0] !== 'v') return;
  const [, chatId, userId, verdict, triesRaw] = parts;
  // 只认本人作答（私聊场景下本就只有本人可见，这里再兜一道）
  if (String(cb.from?.id) !== userId) {
    return tg('answerCallbackQuery', { callback_query_id: cb.id, text: '这不是你的验证', show_alert: true });
  }
  const tries = Number(triesRaw) || 0;

  if (verdict === 'ok') {
    const r = await tg('approveChatJoinRequest', { chat_id: chatId, user_id: userId });
    // 重复处理（作业被取消导致更新重放）时 Telegram 会报错，属正常，不当失败
    await tg('answerCallbackQuery', { callback_query_id: cb.id, text: r.ok ? '验证通过 ✅' : '已处理过' });
    return tg('editMessageText', {
      chat_id: cb.message.chat.id,
      message_id: cb.message.message_id,
      text: '✅ 验证通过，已放行，欢迎加入！',
    });
  }

  const used = tries + 1;
  if (used >= MAX_TRIES) {
    await tg('declineChatJoinRequest', { chat_id: chatId, user_id: userId });
    await tg('answerCallbackQuery', { callback_query_id: cb.id, text: '答错次数过多' });
    return tg('editMessageText', {
      chat_id: cb.message.chat.id,
      message_id: cb.message.message_id,
      text: `❌ 答错 ${MAX_TRIES} 次，本次申请已被拒绝。\n你可以稍后重新申请入群再试一次。`,
    });
  }
  // 还有机会：换一道新题继续
  const { q, markup } = captchaKeyboard(chatId, userId, used);
  await tg('answerCallbackQuery', { callback_query_id: cb.id, text: `答错了，还有 ${MAX_TRIES - used} 次机会` });
  return tg('editMessageText', {
    chat_id: cb.message.chat.id,
    message_id: cb.message.message_id,
    text: captchaText(q, used),
    reply_markup: markup,
  });
}

(async () => {
  // 短轮询拉取待处理更新。⚠️ 未列入 allowed_updates 的类型会被 Telegram 直接丢弃（不是排队等取），
  // 故入群验证所需的 chat_join_request / callback_query 必须显式订阅。
  const WANT = ['message', 'chat_join_request', 'callback_query'];
  const res = await (
    await fetch(tgUrl('getUpdates') + '?timeout=0&allowed_updates=' + encodeURIComponent(JSON.stringify(WANT)))
  ).json();
  if (!res.ok) {
    console.error('getUpdates 失败:', JSON.stringify(res));
    process.exit(res.error_code === 409 ? 0 : 1); // 409 = 设了 webhook，与轮询冲突，直接退出
  }
  const updates = res.result || [];
  let maxId = 0;
  for (const u of updates) {
    maxId = Math.max(maxId, u.update_id);

    // 入群申请 / 验证按钮：单条失败不能拖垮整批（否则后面的更新也确认不掉，下次全部重放）
    if (u.chat_join_request) {
      try {
        await onJoinRequest(u.chat_join_request);
      } catch (e) {
        console.error('join_request 处理失败:', e?.message || e);
      }
      continue;
    }
    if (u.callback_query) {
      try {
        await onCallback(u.callback_query);
      } catch (e) {
        console.error('callback 处理失败:', e?.message || e);
      }
      continue;
    }

    const m = u.message;
    if (!m || !m.text) continue;
    const text = m.text.trim();
    if (!text.startsWith('/')) continue;
    const parts = text.split(/\s+/);
    const cmd = parts[0].split('@')[0].toLowerCase(); // 去掉 @botname 后缀
    const arg = parts.slice(1).join(' ').trim();
    try {
      await handle(cmd, arg, m.chat.id);
    } catch (e) {
      await send(m.chat.id, '⚠️ 出错了：' + (e?.message || e));
    }
  }
  // 向 Telegram 确认已处理（清掉这些更新，下次不再返回）
  if (maxId) {
    await fetch(tgUrl('getUpdates') + `?offset=${maxId + 1}&timeout=0`);
  }
  console.log(`processed ${updates.length} update(s)`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});

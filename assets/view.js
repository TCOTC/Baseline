/* 基线的窗口行为。
 *
 * Rust 渲染整页，只有时间线这一个列表交给这里。原因只有一个：**虚拟滚动**。
 * 它要知道滚到哪儿了、总共多高、该画哪几十行，而 Rust 那边出的是一整串静态 HTML，
 * 这件事插不进去。行的 class 仍然全部来自 view.css，所以样式还是只有一份——
 * 搬过来的只是行的拼装。
 *
 * 其余都是窗口外壳的事：顶栏按钮、底部输入框、新建目标的对话。
 * 产品逻辑（什么算推进、累积怎么算、快照什么时候冻结）一行都不在这里。
 */
(function () {
  'use strict';

  // ---------------------------------------------------------------- 基础

  // 窗口没有系统边框也就没有开发工具，异常不主动送出来就等于不存在。
  function report(what, e) {
    var msg = what + ': ' + ((e && (e.stack || e.message)) || e);
    try {
      if (window.__TAURI__) {
        fetch('/__jslog', { method: 'POST', body: msg, keepalive: true });
      } else {
        if (window.console) console.error(msg);
      }
    } catch (_) {}
  }
  window.addEventListener('error', function (e) { report('window.onerror', e.message); });
  window.addEventListener('unhandledrejection', function (e) { report('unhandled', e.reason); });

  var T = window.__TAURI__;
  function invoke(cmd, args) {
    if (!T || !T.core) return Promise.reject(new Error('不在窗口里，命令发不出去'));
    return T.core.invoke(cmd, args);
  }
  function $(id) { return document.getElementById(id); }
  function esc(s) {
    return String(s).replace(/[&<>"]/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
    });
  }

  // ---------------------------------------------------------------- 顶栏

  function initTitleBar() {
    if (!T || !T.window) return;
    var min = $('w-min'), max = $('w-max'), close = $('w-close');
    if (!min || !max || !close) return; // 导出的单文件没有顶栏
    var w = T.window.getCurrentWindow();

    // 最大化之后那个方框必须变成「还原」，否则它就在骗人。
    function sync() {
      w.isMaximized().then(function (on) {
        max.textContent = on ? '\uE923' : '\uE922';
      }).catch(function (e) { report('isMaximized', e); });
    }
    min.onclick = function () { w.minimize().catch(function (e) { report('minimize', e); }); };
    // 用 maximize/unmaximize 而不是 toggleMaximize：图标要跟着状态走，
    // 而状态本来就得查一次，顺带把「查」和「改」绑在同一次判断里。
    max.onclick = function () {
      w.isMaximized().then(function (on) {
        return on ? w.unmaximize() : w.maximize();
      }).catch(function (e) { report('maximize', e); });
    };
    close.onclick = function () { w.close().catch(function (e) { report('close', e); }); };
    w.onResized(sync);
    sync();
  }

  // ---------------------------------------------------------------- 流水

  /* 行高必须是固定值——虚拟滚动靠它算总高和偏移。值只有 view.css 一份，
     这里读出来用，两边不会各写一份。 */
  var ROW = { d: 34, e: 46 };
  var OVERSCAN = 8;

  var logEl, canvasEl, rowsEl, rows = [], offsets = [], lastKey = '';

  function readRowHeights() {
    var cs = getComputedStyle(document.documentElement);
    var d = parseFloat(cs.getPropertyValue('--row-day'));
    var e = parseFloat(cs.getPropertyValue('--row-entry'));
    // 解析不出来就退回默认值：宁可行高差几像素，也不要整条流水不显示。
    if (isFinite(d) && d > 0) ROW.d = d;
    if (isFinite(e) && e > 0) ROW.e = e;
  }

  function rowHeight(r) { return r.k === 'd' ? ROW.d : ROW.e; }

  function rowHtml(r) {
    if (r.k === 'd') {
      // 具体年月日在 title 上，悬停才出现。看得见的是「哪天」+「周几」。
      return '<div class="day" title="' + esc(r.title || '') + '"><div class="dlabel">' +
        '<span class="dl">' + esc(r.label) + '</span>' +
        (r.wd ? '<span class="dw">' + esc(r.wd) + '</span>' : '') + '</div></div>';
    }
    // 没关联目标的记录：标签安静一点。它确实发生过，只是还没归到哪条线上。
    return '<div class="entry' + (r.linked ? '' : ' unlinked') + '">' +
      '<div class="time">' + esc(r.time) + '</div>' +
      '<div class="rail"><span class="dot ' + esc(r.color) + '"></span></div>' +
      '<div class="body"><div class="act" title="' + esc(r.text) + '">' + esc(r.text) + '</div>' +
      '<div class="sub">' + esc(r.sub) + '</div></div>' +
      '<div class="tags"><span class="chip ' + esc(r.color) + '">' + esc(r.goal) +
      '</span></div></div>';
  }

  function build() {
    offsets = new Array(rows.length + 1);
    offsets[0] = 0;
    for (var i = 0; i < rows.length; i++) offsets[i + 1] = offsets[i] + rowHeight(rows[i]);
    canvasEl.style.height = offsets[rows.length] + 'px';
  }

  // 二分找某一行。行有几千条时线性扫描会把每一次滚动都拖慢。
  function indexAt(y) {
    var lo = 0, hi = rows.length;
    while (lo < hi) {
      var mid = (lo + hi) >> 1;
      if (offsets[mid + 1] <= y) lo = mid + 1; else hi = mid;
    }
    return Math.max(0, Math.min(lo, rows.length - 1));
  }

  var pending = false;
  function paint() {
    pending = false;
    if (!rows.length) return;
    var top = logEl.scrollTop, vh = logEl.clientHeight;
    var start = Math.max(0, indexAt(top) - OVERSCAN);
    var end = Math.min(rows.length, indexAt(top + vh) + 1 + OVERSCAN);
    var key = start + ':' + end;
    if (key === lastKey) return; // 可见范围没变就不重排
    lastKey = key;
    var html = '';
    for (var i = start; i < end; i++) html += rowHtml(rows[i]);
    rowsEl.style.transform = 'translateY(' + offsets[start] + 'px)';
    rowsEl.innerHTML = html;
  }

  function schedule() {
    if (pending) return;
    pending = true;
    requestAnimationFrame(paint);
  }

  function scrollToBottom() {
    logEl.scrollTop = logEl.scrollHeight;
    paint();
  }

  function initLog() {
    logEl = $('log');
    canvasEl = $('log-canvas');
    rowsEl = $('log-rows');
    if (!logEl || !canvasEl || !rowsEl) return;
    readRowHeights();

    try {
      rows = JSON.parse($('log-data').textContent) || [];
    } catch (e) {
      report('log-data 解析失败', e);
      rows = [];
    }

    if (!rows.length) {
      canvasEl.style.height = 'auto';
      rowsEl.style.position = 'static';
      rowsEl.innerHTML = '<div class="log-hint">还没有任何记录。<br>' +
        '下面那个输入框里写一句，回车——记的是「刚做了什么」。</div>';
      return;
    }

    build();
    // 下新上旧：一进来就该看到最新的那条，所以先落到底。
    scrollToBottom();
    logEl.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', function () { lastKey = ''; schedule(); });
  }

  // ---------------------------------------------------------------- 输入框

  function initComposer() {
    var form = $('composer');
    if (!form) return; // 导出的单文件没有输入框
    var input = $('composer-input');
    var goalField = $('composer-goal');
    var gbtn = $('gbtn');
    var glabel = $('gbtn-label');
    var gmenu = $('gmenu');

    // 默认两行、随内容长高。多行是要能打的——一句话写不下的时候，
    // 硬塞进一行会让人写得更短，而备注是这条流水上唯一有信息量的东西。
    var MAX_H = 160;
    function grow() {
      input.style.height = 'auto';
      input.style.height = Math.min(input.scrollHeight, MAX_H) + 'px';
    }
    input.addEventListener('input', grow);
    input.dataset.ph = input.placeholder; // 出错提示要能还原回去
    grow();

    // ---- 记到哪个目标 ----
    //
    // **默认不关联。** 记下来是第一步，归到哪个目标是第二步；逼着先选目标，
    // 等于在「我还不知道这算推进什么」的时候替人做决定。
    // 选没选只改一个隐藏域，提交语义一样：空值就是不关联。
    function closeMenu() { if (gmenu) gmenu.hidden = true; }

    if (gbtn && gmenu) {
      gbtn.addEventListener('click', function () { gmenu.hidden = !gmenu.hidden; });
      gmenu.addEventListener('click', function (ev) {
        var b = ev.target.closest('.gopt');
        if (!b) return;
        Array.prototype.forEach.call(gmenu.children, function (c) { c.classList.remove('on'); });
        b.classList.add('on');
        goalField.value = b.dataset.goal || '';
        if (glabel) glabel.textContent = b.dataset.label || '';
        var dot = gbtn.querySelector('.dot');
        if (dot) dot.className = 'dot ' + (b.dataset.color || 'none');
        closeMenu();
        input.focus();
      });
      // 点别处、按 Esc 都收起来：一个不会消失的浮层会挡住它下面的东西。
      document.addEventListener('click', function (ev) {
        if (!gbtn.contains(ev.target) && !gmenu.contains(ev.target)) closeMenu();
      });
      document.addEventListener('keydown', function (ev) {
        if (ev.key === 'Escape') closeMenu();
      });
    }

    function fail(e) {
      form.classList.remove('busy');
      input.disabled = false;
      report('add_checkin', e);
      input.placeholder = '没记上：' + ((e && e.message) || e);
      form.classList.add('broke');
      setTimeout(function () {
        form.classList.remove('broke');
        input.placeholder = input.dataset.ph || '';
        grow();
      }, 4000);
    }

    function send() {
      var text = input.value.trim();
      if (!text || form.classList.contains('busy')) return;
      form.classList.add('busy');
      input.disabled = true;
      // 空字符串 -> null：Tauri 那边收到 None，这条记录不进任何曲线。
      var gid = goalField.value ? Number(goalField.value) : null;
      invoke('add_checkin', { goalId: gid, note: text })
        .then(function () {
          // 提交后整页重来：Rust 会重算今天的快照，流水重新落到底、输入框重新聚焦。
          // 不做局部插入——那样「卡片上的数字」和「曲线末端」就要在两边各算一次。
          location.reload();
        })
        .catch(fail);
    }

    form.addEventListener('submit', function (ev) { ev.preventDefault(); send(); });
    // 回车提交、Shift+回车换行。支持多行不该让「写完一句回车」变成按两个键。
    input.addEventListener('keydown', function (ev) {
      if (ev.key === 'Enter' && !ev.shiftKey) {
        ev.preventDefault();
        send();
      }
    });

    focusComposer();
  }

  function focusComposer() {
    var input = $('composer-input');
    if (!input || input.disabled) return;
    try { input.focus(); } catch (_) {}
  }
  // 托盘左键点开窗口之后由外壳调用，把光标直接放进输入框。
  window.__blFocusComposer = focusComposer;

  // ---------------------------------------------------------------- 新建目标的对话

  /* 骨架是固定的四步 + 一步收口。
   *
   * 为什么要固定：设计文档 §11.1 那条「3 轮上限怎么定」之所以难调，
   * 是因为自由聊天只能靠 prompt 求它自觉。步数就是步数，就不用调了。
   * 而且 **「什么不算」这一问永远不会被跳过**——它是最容易跳过、也最值钱的一问，
   * 交给自由聊天就是碰运气。
   *
   * AI 在这里没有位置：对话要问什么是产品定死的，判据（≥1 条来源、全部可计算、
   * 当场算出当前值）也全是机械的。等真需要把一句话拆成几条来源时再说。 */
  var KINDS = [
    { k: 'manual_checkin', name: '手工打卡', desc: '我自己记一次。比如「读完一章算一次」', need: false },
    { k: 'git_commits', name: 'git 提交', desc: '某个仓库的提交数。还没接入，先登记规则', need: true, ask: '哪个仓库？（先填名字，接上之后才会计数）' },
    { k: 'external_metric', name: '外部数据', desc: '读别处已经记着的数，比如复习记录', need: true, ask: '读哪份数据？（先填标识，接上之后才会计数）' },
    { k: 'derived', name: '组合', desc: '上面几条按公式算', need: false }
  ];

  var dlgEl, d = null;

  var STEP_NAMES = ['想推进什么', '为什么是现在', '什么算推进它', '什么不算', '落到哪种来源'];

  function dlgHead() {
    var h = '<div class="dlg-steps">';
    for (var i = 0; i < STEP_NAMES.length; i++) {
      var cls = i < d.step ? 'done' : (i === d.step ? 'now' : '');
      h += '<div class="' + cls + '">' + (i + 1) + ' · ' + esc(STEP_NAMES[i]) + '</div>';
    }
    return h + '</div>';
  }

  function draw() {
    var h = dlgHead();
    var i;
    // 回车该干什么，由状态说了算。**绑在容器上、只绑一次**——
    // 绑在每个重建出来的输入框上，重建一次监听就没了（踩过：文字进去了，
    // 回车没反应，看着像「键盘不灵」）。
    d.enter = next;

    if (d.step === 0) {
      h += '<h2>你想推进什么？</h2>' +
        '<p class="note">一句话就行。它会是左栏那张卡的标题。</p>' +
        '<input type="text" id="dlg-in" maxlength="40" placeholder="比如：打牢计算机基础">' +
        dlgNav();
    } else if (d.step === 1) {
      h += '<h2>为什么是现在？</h2>' +
        '<p class="note">遇到什么事了，还是在准备什么？<br>' +
        '这一问是为了推出「什么算推进它」，<b>不是问你感受</b>——所以只有在你上一句太宽、' +
        '推不出规则的时候才问。</p>' +
        '<input type="text" id="dlg-in" maxlength="120" placeholder="比如：想做出自己的产品，但基础不够">' +
        dlgNav();
    } else if (d.step === 2) {
      h += '<h2>什么算推进它？</h2>' +
        '<p class="note">越具体越好。一句能装下任何活动的规则等于没有规则——' +
        '那样的曲线会一直涨，也就永远不告诉你偏了。</p>' +
        '<input type="text" id="dlg-in" maxlength="120" placeholder="比如：读完一章，或做完一章题，算一次">' +
        dlgNav();
    } else if (d.step === 3) {
      h += '<h2>什么不算？</h2>' +
        '<p class="note">最容易跳过、也最值钱的一问。<br>' +
        '同一件事看起来像在推进它、其实不算的，是什么？<br>' +
        '（例：在 Learn-English 上写代码不算推进「英语」——只算复习记录。<br>' +
        '不划这条边界，那条曲线会假装在涨。）</p>' +
        '<input type="text" id="dlg-in" maxlength="120" placeholder="比如：在它上面写代码、改功能，不算">' +
        '<div class="row" style="margin-bottom:10px">' +
        '<button type="button" id="dlg-skip">想不出来，先跳过</button></div>' +
        dlgNav();
    } else if (d.step === 4) {
      h += '<h2>这条规则落到哪种来源？</h2>' +
        '<p class="note">四种，是上限。落到别的上去，它就开始变成另一件东西了。</p>';
      for (i = 0; i < KINDS.length; i++) {
        h += '<button type="button" class="kind" data-kind="' + KINDS[i].k + '">' +
          '<b>' + esc(KINDS[i].name) + '</b><span>' + esc(KINDS[i].desc) + '</span></button>';
      }
      h += '<div class="row" style="margin-top:12px"><button type="button" id="dlg-cancel">取消</button></div>';
      if (d.problem) h += '<div class="cur bad">' + esc(d.problem) + '</div>';
    } else if (d.step === 5) {
      var k = kindOf(d.kind);
      h += '<h2>确认一下</h2>' +
        '<div class="answer"><b>' + esc(d.title) + '</b></div>' +
        (d.why ? '<div class="answer">为什么：' + esc(d.why) + '</div>' : '') +
        '<div class="answer">算推进：' + esc(d.rule) + '</div>' +
        '<div class="answer">不算：' + (d.notrule ? esc(d.notrule) : '<b>没划</b>') + '</div>' +
        '<div class="answer">来源：' + esc(k.name) + (d.target ? ' · ' + esc(d.target) : '') + '</div>' +
        (d.notrule ? '' : '<div class="cur bad">没划边界。以后出现「看起来像在推进它、其实不算」的' +
          '活动时，这条曲线会说谎——而它会理直气壮地涨。</div>') +
        '<div class="row" style="margin-top:16px">' +
        '<button type="button" class="primary" id="dlg-ok">建这个目标</button>' +
        '<button type="button" id="dlg-cancel">取消</button></div>' +
        (d.problem ? '<div class="cur bad">' + esc(d.problem) + '</div>' : '');
    } else if (d.step === 6) {
      h += '<h2>建好了</h2>' +
        '<div class="cur">当前值 <b>' + d.value + '</b> 次<br>' +
        (d.value === 0
          ? '还是 0。要么规则写错了，要么这条线真的还没动——两种都值得知道。'
          : '规则一写完就算出了数，说明它是可计算的。') +
        '</div>' +
        '<div class="row" style="margin-top:16px">' +
        '<button type="button" class="primary" id="dlg-done">好</button></div>';
    }

    dlgEl.innerHTML = h;
    var input = $('dlg-in');
    if (input) {
      input.value = d['v' + d.step] || '';
      focusInput();
    }
    var skip = $('dlg-skip');
    if (skip) skip.onclick = function () { d.notrule = ''; d.step = 4; draw(); };
    // 下一步 / 上一步必须有接线。少了这两行，按钮画出来了、点了没反应——
    // 而回车又能走通，所以看起来「界面是好的，只是键盘不灵」。踩过。
    var nextBtn = $('dlg-next');
    if (nextBtn) nextBtn.onclick = next;
    var backBtn = $('dlg-back');
    if (backBtn) {
      backBtn.onclick = function () {
        if (d.step > 0) { d.problem = ''; d.step -= 1; draw(); }
      };
    }
    var ok = $('dlg-ok');
    if (ok) ok.onclick = function () { create(); };
    var done = $('dlg-done');
    if (done) done.onclick = function () { location.reload(); };
    var cancel = $('dlg-cancel');
    if (cancel) cancel.onclick = closeDialog;
    Array.prototype.forEach.call(dlgEl.querySelectorAll('button.kind'), function (b) {
      b.onclick = function () { pickKind(b.dataset.kind); };
    });
  }

  function kindOf(k) {
    for (var i = 0; i < KINDS.length; i++) if (KINDS[i].k === k) return KINDS[i];
    return KINDS[0];
  }

  /// 把光标放进当前这一步的输入框。
  ///
  /// 不能在写完 innerHTML 之后立刻 focus：那一刻元素还没有完成布局，
  /// focus() 会被静默忽略——**不报错，只是没生效**，看起来像「键盘不灵」。
  /// 所以延到下一帧，并且再补一次：一帧之后仍然没拿到的，多半是被别的东西抢走了。
  function focusInput() {
    function go() {
      var input = $('dlg-in');
      if (!input || document.activeElement === input) return;
      try { input.focus(); } catch (_) {}
    }
    requestAnimationFrame(go);
    setTimeout(go, 80);
  }

  function dlgNav() {
    return '<div class="row">' +
      '<button type="button" class="primary" id="dlg-next">下一步</button>' +
      (d.step > 0 ? '<button type="button" id="dlg-back">上一步</button>' : '') +
      '<button type="button" id="dlg-cancel">取消</button></div>' +
      (d.problem ? '<div class="cur bad">' + esc(d.problem) + '</div>' : '');
  }

  function next() {
    var v = ($('dlg-in') && $('dlg-in').value || '').trim();
    if (d.step === 0) {
      if (!v) { d.problem = '总得先有个名字。'; draw(); return; }
      d.title = v;
    } else if (d.step === 1) {
      d.why = v; // 可以不填：不是每个目标都需要问目的
    } else if (d.step === 2) {
      if (!v) {
        // 这是「必须再问」的落点：收不出一条可计算的规则，就不该往下走。
        d.problem = '这条填不了。没有它，这个目标给不出「什么算推进它」——' +
          '曲线永远不会动，它也就不是目标，只是一句愿望。';
        draw();
        return;
      }
      d.rule = v;
    } else if (d.step === 3) {
      d.notrule = v;
    }
    d.problem = '';
    d.step += 1;
    draw();
  }

  function pickKind(k) {
    var spec = kindOf(k);
    if (spec.need) {
      // 需要 target 的来源：先问清对象。空着就等于「什么都算」，
      // 那正是设计文档里点名的坏规则。
      d.kind = k;
      d.enter = takeTarget;
      dlgEl.innerHTML = dlgHead() + '<h2>' + esc(spec.name) + ' · 具体是哪个？</h2>' +
        '<p class="note">' + esc(spec.ask) + '</p>' +
        '<input type="text" id="dlg-in" maxlength="120">' +
        '<div class="row"><button type="button" class="primary" id="dlg-next">下一步</button>' +
        '<button type="button" id="dlg-back">上一步</button>' +
        '<button type="button" id="dlg-cancel">取消</button></div>' +
        (d.problem ? '<div class="cur bad">' + esc(d.problem) + '</div>' : '');
      var input = $('dlg-in');
      input.value = d.target || '';
      focusInput();
      $('dlg-next').onclick = takeTarget;
      $('dlg-back').onclick = function () { d.step = 4; d.problem = ''; draw(); };
      $('dlg-cancel').onclick = closeDialog;
      return;
    }
    d.kind = k;
    d.target = '';
    d.problem = '';
    d.step = 5;
    draw();
  }

  function takeTarget() {
    var v = ($('dlg-in') && $('dlg-in').value || '').trim();
    if (!v) {
      d.problem = '空着就等于「什么都算」——那条规则会把所有活动都装进来，' +
        '曲线永远在涨，也就不告诉你偏了。';
      pickKind(d.kind);
      return;
    }
    d.target = v;
    d.problem = '';
    d.step = 5;
    draw();
  }

  function create() {
    var btn = $('dlg-ok');
    if (btn) btn.disabled = true;
    invoke('add_goal', {
      title: d.title, why: d.why, rationale: d.rule, kind: d.kind, target: d.target
    }).then(function (value) {
      d.value = value;
      d.problem = '';
      d.step = 6;
      draw();
    }).catch(function (e) {
      if (btn) btn.disabled = false;
      report('add_goal', e);
      d.problem = '没建成：' + ((e && e.message) || e);
      draw();
    });
  }

  function openDialog() {
    d = { step: 0, title: '', why: '', rule: '', notrule: '', kind: '', target: '',
          problem: '', enter: null, value: 0 };
    dlgEl.hidden = false;
    if (logEl) logEl.hidden = true;
    var c = $('composer');
    if (c) c.hidden = true;
    draw();
  }

  function closeDialog() {
    d = null;
    dlgEl.hidden = true;
    dlgEl.innerHTML = '';
    if (logEl) logEl.hidden = false;
    var c = $('composer');
    if (c) c.hidden = false;
    focusComposer();
  }

  function initAddGoal() {
    dlgEl = $('dlg');
    var btn = $('addgoal');
    if (!dlgEl || !btn || !T) return;
    btn.addEventListener('click', openDialog);
    dlgEl.addEventListener('keydown', function (ev) {
      if (ev.key !== 'Enter' || !d) return;
      var t = ev.target;
      if (!t || t.tagName !== 'INPUT') return;
      ev.preventDefault();
      if (d.enter) d.enter();
    });
  }

  // ---------------------------------------------------------------- 目标详情

  /* 点卡片进来的那一屏。Rust 已经把所有状态（读的、编辑的、归档的、加规则的）
     一次渲染好了，这里只负责切显隐和把结果发回去。
     所以「一个目标长什么样」仍然只有一处定义。 */
  function initDetail() {
    var detail = document.querySelector('.detail');
    if (!detail || !T) return; // 不是详情页，或者不在窗口里
    var id = Number(detail.dataset.goal);

    var view = $('dview'), edit = $('deditform'), arch = $('darchform'), addr = $('addrform');
    var err = $('derr');

    function fail(e) {
      report('detail', e);
      if (!err) return;
      err.textContent = (e && e.message) || String(e);
      err.hidden = false;
    }
    function ok() { location.reload(); }
    function back() { location.href = '/'; }

    // ---- 改目标 ----
    $('dedit').onclick = function () {
      view.hidden = true; edit.hidden = false;
      $('etitle').focus();
    };
    $('ecancel').onclick = function () { edit.hidden = true; view.hidden = false; };
    $('esave').onclick = function () {
      var title = $('etitle').value.trim();
      if (!title) { fail(new Error('目标得有个名字')); return; }
      invoke('update_goal', { id: id, title: title, why: $('ewhy').value }).then(ok).catch(fail);
    };

    // ---- 归档：必须写一句原因 ----
    $('darch').onclick = function () { arch.hidden = false; $('areason').focus(); };
    $('acancel').onclick = function () { arch.hidden = true; };
    $('asave').onclick = function () {
      var reason = $('areason').value.trim();
      if (!reason) {
        fail(new Error('归档要写一句原因——那是三个月后唯一能回看的东西'));
        return;
      }
      // 归档完它就不在列表里了，回主视图，别停在一个已经归档的目标上。
      invoke('archive_goal', { id: id, reason: reason }).then(back).catch(fail);
    };

    // ---- 删除：只有一条记录都没有时才可点（Rust 那边已经按这个置灰了）----
    var del = $('ddel');
    if (del && !del.disabled) {
      del.onclick = function () {
        // 不用 window.confirm：原生弹窗在这扇无边框窗口里很突兀，而且它挡住 JS 线程。
        // 就地变成两步确认——顺带让人看清自己正在确认什么。
        if (del.dataset.armed !== '1') {
          del.dataset.armed = '1';
          del.textContent = '真的删？再点一下';
          del.classList.add('armed');
          setTimeout(function () {
            if (del.dataset.armed === '1') {
              del.dataset.armed = '';
              del.textContent = '删除';
              del.classList.remove('armed');
            }
          }, 4000);
          return;
        }
        invoke('delete_goal', { id: id }).then(back).catch(fail);
      };
    }

    // ---- 规则的增删 ----
    Array.prototype.forEach.call(document.querySelectorAll('.rule .x'), function (b) {
      b.onclick = function () {
        invoke('delete_source', { id: Number(b.dataset.src) }).then(ok).catch(fail);
      };
    });

    $('addrbtn').onclick = function () { addr.hidden = !addr.hidden; };
    $('arcancel').onclick = function () { addr.hidden = true; };

    var chosen = '';
    Array.prototype.forEach.call(document.querySelectorAll('#addrkinds .kind'), function (b) {
      b.onclick = function () {
        chosen = b.dataset.kind;
        Array.prototype.forEach.call(document.querySelectorAll('#addrkinds .kind'), function (x) {
          x.classList.remove('on');
        });
        b.classList.add('on');
        // 需要对象的来源（git 仓库、外部数据）必须问清是哪个。
        // 空着就等于「什么都算」，那正是设计文档点名的坏规则。
        var need = chosen === 'git_commits' || chosen === 'external_metric';
        $('addrtarget').hidden = !need;
        $('addrhint').textContent =
          chosen === 'git_commits' ? '哪个仓库？' : '读哪份数据？';
      };
    });

    $('arsave').onclick = function () {
      if (!chosen) { fail(new Error('先选一种来源')); return; }
      var t = $('artarget').value.trim();
      if ((chosen === 'git_commits' || chosen === 'external_metric') && !t) {
        fail(new Error('空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨'));
        return;
      }
      invoke('add_source', {
        goalId: id, kind: chosen, target: t, rationale: $('arrationale').value
      }).then(ok).catch(fail);
    };
  }

  // ---------------------------------------------------------------- 启动

  try {
    initTitleBar();
    initLog();
    initComposer();
    initAddGoal();
    initDetail();
    // 每次加载报一行。用来区分「窗口起来了」和「窗口起来了但页面是空的」——
    // 这两种情况从外面看一模一样，处置却完全相反。
    // 两个行数是虚拟滚动的证据：总行数很多，真正进了 DOM 的只有可见的那几十行。
    if (T) {
      report('page', document.querySelectorAll('.card').length + ' cards, ' +
        rows.length + ' log rows, ' + (rowsEl ? rowsEl.children.length : 0) + ' in dom');
    }
  } catch (e) {
    report('启动失败', e);
  }
})();

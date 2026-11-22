<?php
// Public allergen page. The label app keeps data.json up to date through update.php.
header('Content-Type: text/html; charset=utf-8');
header('Cache-Control: no-cache');

function h($s) { return htmlspecialchars((string)$s, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8'); }

$data = null;
$raw = @file_get_contents(__DIR__ . '/data.json');
if ($raw !== false) { $data = json_decode($raw, true); }
$products = is_array($data['products'] ?? null) ? $data['products'] : [];
$allergens = [];
foreach (($data['allergens'] ?? []) as $a) { $allergens[(string)$a['key']] = (string)$a['name']; }

// Only offer the allergens that actually appear, in the standard order.
$used = [];
foreach ($products as $p) {
    foreach (array_merge($p['contains'] ?? [], $p['may'] ?? []) as $k) { $used[$k] = true; }
}
$filters = array_filter($allergens, function ($k) use ($used) { return isset($used[$k]); }, ARRAY_FILTER_USE_KEY);

function names($keys, $allergens) {
    $out = [];
    foreach ($keys as $k) { $out[] = $allergens[$k] ?? $k; }
    return $out;
}
function ingredients_html($ings) {
    $parts = [];
    foreach ($ings as $cell) {
        $s = '';
        foreach ($cell as $r) {
            $t = h($r['t'] ?? '');
            $s .= !empty($r['b']) ? '<strong>' . $t . '</strong>' : $t;
        }
        $parts[] = trim($s);
    }
    return implode(', ', array_filter($parts, 'strlen'));
}
$business = trim((string)($data['business_name'] ?? 'PoFfles'));
?>
<!doctype html>
<html lang="en-GB">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>Allergens – The Cake Pantry by PoFfles</title>
<style>
  :root { --bg: #faf7f4; --card: #fff; --text: #22201d; --muted: #6b665f; --line: #e6dfd7; --accent: #c2552e;
    --warn-bg: #fff4e0; --warn: #8a5300; --bad-bg: #fde8e6; --bad: #a3271d; --ok: #2e7d4f; }
  @media (prefers-color-scheme: dark) {
    :root { --bg: #1b1a19; --card: #262523; --text: #eeebe6; --muted: #aaa49b; --line: #3a3835;
      --warn-bg: #3a2c12; --warn: #f0c070; --bad-bg: #3d1d1a; --bad: #f19a90; }
  }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--bg); color: var(--text);
    font: 17px/1.5 system-ui, -apple-system, "Segoe UI", Roboto, Arial, sans-serif; }
  main { max-width: 760px; margin: 0 auto; padding: 18px 16px 40px; }
  h1 { font-size: 1.7rem; margin: 0; }
  .sub { color: var(--muted); margin: 2px 0 16px; }
  .notice { background: var(--card); border: 1px solid var(--line); border-left: 4px solid var(--accent);
    border-radius: 10px; padding: 12px 14px; margin: 0 0 18px; font-size: .95rem; }
  .filter h2 { font-size: 1rem; margin: 0 0 8px; }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 8px; }
  .chip { font: inherit; font-size: .95rem; padding: 8px 14px; min-height: 44px; border-radius: 999px; cursor: pointer;
    border: 1px solid var(--line); background: var(--card); color: var(--text); }
  .chip[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
  .status { color: var(--muted); font-size: .9rem; min-height: 1.4em; margin: 4px 0 14px; }
  input[type=search] { width: 100%; font: inherit; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--line);
    background: var(--card); color: var(--text); margin-bottom: 14px; }
  .product { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 14px 16px; margin-bottom: 12px; }
  .product h3 { margin: 0 0 6px; font-size: 1.2rem; }
  .tags { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 8px; }
  .tag { font-size: .85rem; padding: 2px 10px; border-radius: 999px; background: var(--bad-bg); color: var(--bad); font-weight: 600; }
  .tag.may { background: var(--warn-bg); color: var(--warn); font-weight: 500; }
  .tag.none { background: transparent; color: var(--ok); padding-left: 0; }
  .ings { margin: 0; }
  .ings strong { font-weight: 800; }
  .may-line { margin: 6px 0 0; color: var(--muted); font-size: .95rem; }
  .pending { color: var(--warn); }
  .product.flag { border-color: var(--warn); }
  .product.flag .flag-note { display: block; }
  .flag-note { display: none; margin: 8px 0 0; color: var(--warn); font-weight: 600; font-size: .95rem; }
  footer { margin-top: 26px; color: var(--muted); font-size: .9rem; }
  footer p { margin: 0 0 8px; }
  [hidden] { display: none !important; }
</style>
</head>
<body>
<main>
  <h1>Allergens</h1>
  <p class="sub">The Cake Pantry by PoFfles</p>

<?php if (!$products): ?>
  <p class="notice">The allergen list is being set up. Please check the label on each product, or ask us.</p>
<?php else: ?>
  <p class="notice">Allergens are shown in <strong>bold</strong> in each ingredients list. Always check the label on the product too, as recipes can change.</p>

<?php if ($filters): ?>
  <section class="filter" aria-label="Filter by allergen">
    <h2>Avoiding something? Tap to hide products that contain it:</h2>
    <div class="chips">
<?php foreach ($filters as $k => $n): ?>
      <button type="button" class="chip" data-key="<?= h($k) ?>" aria-pressed="false"><?= h($n) ?></button>
<?php endforeach; ?>
    </div>
  </section>
<?php endif; ?>
  <p class="status" id="status" aria-live="polite"></p>
<?php if (count($products) > 8): ?>
  <input type="search" id="search" placeholder="Search products" aria-label="Search products">
<?php endif; ?>

  <div id="list">
<?php foreach ($products as $p):
    $contains = $p['contains'] ?? [];
    $may = $p['may'] ?? [];
    $pending = !empty($p['pending']);
?>
    <article class="product" data-name="<?= h(mb_strtolower((string)$p['name'], 'UTF-8')) ?>"
      data-contains="<?= h(implode(' ', $contains)) ?>" data-may="<?= h(implode(' ', $may)) ?>" data-pending="<?= $pending ? '1' : '0' ?>">
      <h3><?= h($p['name']) ?></h3>
<?php if ($pending): ?>
      <p class="pending">We're still checking this product's details. Please read the ingredients on its pack.</p>
<?php else: ?>
      <div class="tags">
<?php if ($contains): foreach (names($contains, $allergens) as $n): ?>
        <span class="tag">Contains <?= h($n) ?></span>
<?php endforeach; else: ?>
        <span class="tag none">None of the 14 main allergens in the ingredients</span>
<?php endif; ?>
<?php foreach (names($may, $allergens) as $n): ?>
        <span class="tag may">May contain <?= h($n) ?></span>
<?php endforeach; ?>
      </div>
      <p class="ings"><span>Ingredients:</span> <?= ingredients_html($p['ingredients'] ?? []) ?>.</p>
<?php if (trim((string)($p['may_contain'] ?? '')) !== ''): ?>
      <p class="may-line">May contain: <?= h($p['may_contain']) ?></p>
<?php endif; ?>
      <p class="flag-note"></p>
<?php endif; ?>
    </article>
<?php endforeach; ?>
  </div>
<?php endif; ?>

  <footer>
<?php if (trim((string)($data['warning'] ?? '')) !== ''): ?>
    <p><?= h($data['warning']) ?></p>
<?php endif; ?>
    <p>If you have a severe allergy, please contact us before eating<?php if (trim((string)($data['contact'] ?? '')) !== ''): ?>: <a href="mailto:<?= h($data['contact']) ?>"><?= h($data['contact']) ?></a><?php endif; ?>.</p>
    <p><?= h($business) ?><?php if (trim((string)($data['address'] ?? '')) !== ''): ?>, <?= h($data['address']) ?><?php endif; ?></p>
<?php if (!empty($data['updated'])): ?>
    <p>Updated <?= h($data['updated']) ?>.</p>
<?php endif; ?>
  </footer>
</main>
<script>
(function () {
  var chips = Array.prototype.slice.call(document.querySelectorAll('.chip'));
  var cards = Array.prototype.slice.call(document.querySelectorAll('.product'));
  var search = document.getElementById('search');
  var status = document.getElementById('status');
  var names = {};
  chips.forEach(function (c) { names[c.dataset.key] = c.textContent; });
  function words(s) { return s ? s.split(' ') : []; }
  function update() {
    var avoid = chips.filter(function (c) { return c.getAttribute('aria-pressed') === 'true'; }).map(function (c) { return c.dataset.key; });
    var q = search ? search.value.trim().toLowerCase() : '';
    var hidden = 0, pendingHidden = 0, shown = 0;
    cards.forEach(function (card) {
      var contains = words(card.dataset.contains), may = words(card.dataset.may);
      var pending = card.dataset.pending === '1';
      var bad = avoid.some(function (k) { return contains.indexOf(k) >= 0; });
      var maybe = avoid.filter(function (k) { return may.indexOf(k) >= 0; });
      var hide = bad || (pending && avoid.length > 0);
      var match = !q || card.dataset.name.indexOf(q) >= 0;
      card.hidden = hide || !match;
      if (hide) { hidden++; if (pending) pendingHidden++; } else if (match) { shown++; }
      card.classList.toggle('flag', !hide && maybe.length > 0);
      var note = card.querySelector('.flag-note');
      if (note) note.textContent = maybe.length ? 'May contain ' + maybe.map(function (k) { return names[k] || k; }).join(', ') + ' – take care.' : '';
    });
    if (!avoid.length) { status.textContent = ''; return; }
    var msg = shown + (shown === 1 ? ' product' : ' products') + ' without ' + avoid.map(function (k) { return names[k]; }).join(' or ') + '.';
    if (pendingHidden) msg += ' ' + pendingHidden + ' not yet checked ' + (pendingHidden === 1 ? 'is' : 'are') + ' hidden too.';
    status.textContent = msg;
  }
  chips.forEach(function (c) {
    c.addEventListener('click', function () {
      c.setAttribute('aria-pressed', c.getAttribute('aria-pressed') === 'true' ? 'false' : 'true');
      update();
    });
  });
  if (search) search.addEventListener('input', update);
})();
</script>
</body>
</html>

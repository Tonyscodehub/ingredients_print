<?php
// Receives the products and allergens from the label app (labels-server) and
// saves them for index.php. Only accepts a POST with the right X-Publish-Key.
header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');
header('X-Robots-Tag: noindex');

function fail($code, $msg) {
    http_response_code($code);
    echo json_encode(['error' => $msg]);
    exit;
}

if (($_SERVER['REQUEST_METHOD'] ?? '') !== 'POST') {
    fail(405, 'this address only accepts updates from the label app');
}

$cfg = @include __DIR__ . '/config.php';
$expected = is_array($cfg) ? (string)($cfg['key'] ?? '') : '';
if (strlen($expected) < 20) {
    fail(500, 'the key in config.php is missing or too short');
}
$given = (string)($_SERVER['HTTP_X_PUBLISH_KEY'] ?? '');
if (!hash_equals($expected, $given)) {
    fail(403, 'wrong key');
}

$body = file_get_contents('php://input', false, null, 0, 4000000);
$data = json_decode($body, true);
if (!is_array($data) || !isset($data['products']) || !is_array($data['products'])) {
    fail(400, 'the update could not be read');
}

$file = __DIR__ . '/data.json';
$tmp = $file . '.tmp';
if (file_put_contents($tmp, $body, LOCK_EX) === false || !rename($tmp, $file)) {
    fail(500, 'the page could not save the update (check folder permissions)');
}
echo json_encode(['ok' => true, 'count' => count($data['products'])]);

<?php
declare(strict_types=1);
require dirname(__DIR__) . '/src/Auth.php';
use Ketchup\Auth;
$count = 0;
function check(bool $condition): void { global $count; if (!$condition) { throw new RuntimeException('Assertion failed'); } $count++; }
function fails(Closure $action): void { try { $action(); } catch (RuntimeException) { check(true); return; } throw new RuntimeException('Expected failure'); }
$config = ['client_id' => 'test-client', 'client_secret' => 'test-secret', 'redirect_uri' => 'http://localhost:8080/auth/google/callback'];
$calls = [];
$transport = function ($method, $url, $fields, $token) use (&$calls) {
    $calls[] = [$method, $url, $fields, $token];
    return $method === 'POST' ? ['access_token' => 'test-access', 'token_type' => 'Bearer'] : ['sub' => 'google-123', 'name' => '<script>', 'email' => 'test@example.com', 'email_verified' => true];
};
$auth = new Auth($config, $transport);
$session = [];
parse_str(parse_url($auth->begin($session, 1000), PHP_URL_QUERY), $params);
check($params['response_type'] === 'code' && $params['scope'] === 'openid email profile');
check($params['code_challenge_method'] === 'S256');
$pending = $session['oauth'];
check($params['code_challenge'] === rtrim(strtr(base64_encode(hash('sha256', $pending['verifier'], true)), '+/', '-_'), '='));
$user = $auth->callback($session, ['state' => $params['state'], 'code' => 'authorization-code'], 1001);
check(!isset($session['oauth']));
check($calls[0][2]['code_verifier'] === $pending['verifier'] && $calls[0][2]['code'] === 'authorization-code');
check($calls[0][2]['redirect_uri'] === $config['redirect_uri'] && $calls[0][2]['client_secret'] === 'test-secret');
check($calls[1][3] === 'test-access' && $user['id'] === 'google-123');
Auth::login($session, $user, 1001);
check(Auth::user($session, 1002) === $user && !isset($session['access_token']));
$csrf = $session['csrf'];
fails(function () use (&$session) { Auth::logout($session, 'wrong'); });
check(Auth::user($session, 1002) !== null);
Auth::logout($session, $csrf);
check($session === []);
fails(function () use ($auth, &$session, $params) { $auth->callback($session, ['state' => $params['state'], 'code' => 'replay'], 1002); });
foreach (['wrong-state', 'expired', 'future', 'denied', 'missing-code'] as $case) {
    $auth->begin($session, 1000);
    $query = ['state' => $session['oauth']['state'], 'code' => 'code'];
    $now = 1001;
    if ($case === 'wrong-state') { $query['state'] = 'wrong'; }
    if ($case === 'expired') { $now = 1600; }
    if ($case === 'future') { $now = 999; }
    if ($case === 'denied') { $query['error'] = 'access_denied'; }
    if ($case === 'missing-code') { unset($query['code']); }
    fails(function () use ($auth, &$session, $query, $now) { $auth->callback($session, $query, $now); });
    check(!isset($session['oauth']));
}
check(count($calls) === 2); // Rejected callbacks never contact Google.
foreach ([fn() => throw new RuntimeException('network failure'), fn() => ['access_token' => ''], fn($method) => $method === 'POST' ? ['access_token' => 'token', 'token_type' => 'Bearer'] : ['email' => 'no-sub']] as $badTransport) {
    $broken = new Auth($config, $badTransport);
    $broken->begin($session, 1000);
    $query = ['state' => $session['oauth']['state'], 'code' => 'code'];
    fails(function () use ($broken, &$session, $query) { $broken->callback($session, $query, 1001); });
    check(!isset($session['user']) && !isset($session['oauth']));
}
Auth::login($session, $user, 1000);
check(Auth::user($session, 29800) === null && $session === []);
fails(function () { $empty = []; (new Auth([], fn() => []))->begin($empty, 1000); });
echo "OK: $count assertions\n";

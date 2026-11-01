<?php
declare(strict_types=1);
// Real HTTP smoke test: production router and PHP session cookies, no provider bypass.
$listener = stream_socket_server('tcp://127.0.0.1:0', $errno, $error);
$address = stream_socket_get_name($listener, false);
fclose($listener);
$root = dirname(__DIR__);
$log = tmpfile();
$process = proc_open([PHP_BINARY, '-S', $address, '-t', $root . '/public', $root . '/public/index.php'], [0 => ['pipe', 'r'], 1 => $log, 2 => $log], $pipes, $root);
if (!is_resource($process)) { throw new RuntimeException('Cannot start PHP server'); }
$cookie = tempnam(sys_get_temp_dir(), 'ketchup-test-');
function request(string $path, string $method = 'GET', array $fields = []): array {
    global $address, $cookie;
    $curl = curl_init('http://' . $address . $path);
    curl_setopt_array($curl, [CURLOPT_RETURNTRANSFER => true, CURLOPT_HEADER => true, CURLOPT_TIMEOUT => 2,
        CURLOPT_COOKIEJAR => $cookie, CURLOPT_COOKIEFILE => $cookie]);
    if ($method === 'POST') { curl_setopt($curl, CURLOPT_POSTFIELDS, http_build_query($fields)); }
    $response = curl_exec($curl);
    return [curl_getinfo($curl, CURLINFO_RESPONSE_CODE), $response];
}
function verify(bool $ok): void { if (!$ok) { throw new RuntimeException('HTTP assertion failed'); } }
try {
    for ($i = 0; $i < 40; $i++) {
        $socket = @stream_socket_client('tcp://' . $address, $errno, $error, 0.1);
        if ($socket) { fclose($socket); break; }
        usleep(50000);
    }
    [$status, $body] = request('/');
    verify($status === 200 && str_contains($body, 'Se connecter avec Google'));
    verify(str_contains(strtolower($body), 'httponly') && str_contains(strtolower($body), 'samesite=lax'));
    verify(str_contains($body, 'Cache-Control: no-store'));
    [$status, $body] = request('/api/session');
    verify($status === 200 && str_contains($body, '"authenticated":false,"user":null'));
    [$status, $body] = request('/auth/google/callback?state=invalid&code=fake');
    verify($status === 400 && str_contains($body, 'invalide ou expirée'));
    [$status] = request('/logout', 'POST', ['csrf' => 'fake']);
    verify($status === 400);
    [$status] = request('/logout');
    verify($status === 405);
    [$status] = request('/missing');
    verify($status === 404);
    [$status] = request('/config.local.php');
    verify($status === 404);
    echo "OK: 9 HTTP checks (router, session cookie, anonymous status, invalid callback, CSRF, methods, document root)\n";
} finally {
    proc_terminate($process);
    proc_close($process);
    fclose($log);
    unlink($cookie);
}

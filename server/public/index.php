<?php
declare(strict_types=1);
require dirname(__DIR__) . '/src/Auth.php';
require dirname(__DIR__) . '/src/GoogleTransport.php';

use Ketchup\Auth;
use Ketchup\GoogleTransport;

$configFile = dirname(__DIR__) . '/config.local.php';
$config = is_file($configFile) ? require $configFile : require dirname(__DIR__) . '/config.example.php';
$redirect = parse_url($config['redirect_uri'] ?? '');
$secure = ($redirect['scheme'] ?? '') === 'https';
if (!$secure && !(($redirect['scheme'] ?? '') === 'http' && in_array($redirect['host'] ?? '', ['localhost', '127.0.0.1'], true))) {
    http_response_code(500);
    exit('La configuration exige HTTPS hors de localhost.');
}
ini_set('session.use_strict_mode', '1');
ini_set('session.use_only_cookies', '1');
session_name('ketchup_session');
session_set_cookie_params(['lifetime' => 0, 'path' => '/', 'secure' => $secure, 'httponly' => true, 'samesite' => 'Lax']);
session_start();
header('Cache-Control: no-store');
header('Referrer-Policy: no-referrer');
header('X-Content-Type-Options: nosniff');
header("Content-Security-Policy: default-src 'none'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'");
$auth = new Auth($config, (new GoogleTransport())(...));
$path = parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);
$method = $_SERVER['REQUEST_METHOD'];
$user = Auth::user($_SESSION, time());
$error = null;
try {
    if ($path === '/auth/google' && $method === 'GET') {
        session_regenerate_id(true);
        header('Location: ' . $auth->begin($_SESSION, time()), true, 302);
        exit;
    }
    if ($path === '/auth/google/callback' && $method === 'GET') {
        $user = $auth->callback($_SESSION, $_GET, time());
        session_regenerate_id(true);
        Auth::login($_SESSION, $user, time());
        header('Location: /', true, 303);
        exit;
    }
    if ($path === '/logout' && $method === 'POST') {
        Auth::logout($_SESSION, $_POST['csrf'] ?? null);
        setcookie(session_name(), '', ['expires' => time() - 3600, 'path' => '/', 'secure' => $secure, 'httponly' => true, 'samesite' => 'Lax']);
        session_destroy();
        header('Location: /', true, 303);
        exit;
    }
    if ($path === '/api/session' && $method === 'GET') {
        header('Content-Type: application/json; charset=utf-8');
        echo json_encode(['authenticated' => $user !== null, 'user' => $user], JSON_THROW_ON_ERROR);
        exit;
    }
    if ($path !== '/' || $method !== 'GET') {
        http_response_code(in_array($path, ['/', '/logout', '/api/session', '/auth/google', '/auth/google/callback'], true) ? 405 : 404);
        exit('Route ou méthode indisponible.');
    }
} catch (RuntimeException $exception) {
    http_response_code(400);
    $error = $exception->getMessage();
}
function escape(string $value): string { return htmlspecialchars($value, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8'); }
header('Content-Type: text/html; charset=utf-8');
?>
<!doctype html>
<html lang="fr"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Ketchup Server — Connexion</title>
<h1>Ketchup Server</h1>
<?php if ($error !== null): ?><p role="alert"><?= escape($error) ?></p><?php endif ?>
<?php if ($user !== null): ?>
<p>Connecté : <?= escape($user['name']) ?> — <?= escape($user['email']) ?></p>
<form method="post" action="/logout"><input type="hidden" name="csrf" value="<?= escape($_SESSION['csrf']) ?>"><button>Se déconnecter</button></form>
<?php else: ?>
<p>Vous n’êtes pas connecté.</p><a href="/auth/google">Se connecter avec Google</a>
<?php endif ?>
</html>

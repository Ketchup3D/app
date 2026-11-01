<?php
declare(strict_types=1);

namespace Ketchup;

use RuntimeException;

final class Auth
{
    public function __construct(private array $config, private \Closure $transport) {}

    public function begin(array &$session, int $now): string
    {
        if (empty($this->config['client_id']) || empty($this->config['client_secret'])) {
            throw new RuntimeException('La connexion Google doit être configurée sur le serveur.');
        }
        $state = bin2hex(random_bytes(32));
        $verifier = bin2hex(random_bytes(32));
        $session['oauth'] = ['state' => $state, 'verifier' => $verifier, 'created' => $now];
        return 'https://accounts.google.com/o/oauth2/v2/auth?' . http_build_query([
            'client_id' => $this->config['client_id'],
            'redirect_uri' => $this->config['redirect_uri'],
            'response_type' => 'code', 'scope' => 'openid email profile',
            'state' => $state,
            'code_challenge' => rtrim(strtr(base64_encode(hash('sha256', $verifier, true)), '+/', '-_'), '='),
            'code_challenge_method' => 'S256',
        ], '', '&', PHP_QUERY_RFC3986);
    }

    public function callback(array &$session, array $query, int $now): array
    {
        $pending = $session['oauth'] ?? null;
        unset($session['oauth']); // Every callback consumes the attempt, including failures.
        if (!is_array($pending) || !is_string($query['state'] ?? null)
            || !hash_equals($pending['state'], $query['state'])
            || $now < $pending['created'] || $now - $pending['created'] >= 600) {
            throw new RuntimeException('Tentative de connexion invalide ou expirée. Réessayez.');
        }
        if (isset($query['error'])) {
            throw new RuntimeException('La connexion Google a été refusée ou annulée.');
        }
        if (!is_string($query['code'] ?? null) || $query['code'] === '') {
            throw new RuntimeException('Code de connexion Google absent.');
        }
        $tokens = ($this->transport)('POST', 'https://oauth2.googleapis.com/token', [
            'client_id' => $this->config['client_id'], 'client_secret' => $this->config['client_secret'],
            'redirect_uri' => $this->config['redirect_uri'], 'grant_type' => 'authorization_code',
            'code' => $query['code'], 'code_verifier' => $pending['verifier'],
        ], null);
        if (!is_string($tokens['access_token'] ?? null) || $tokens['access_token'] === ''
            || !is_string($tokens['token_type'] ?? null)
            || strcasecmp($tokens['token_type'], 'Bearer') !== 0) {
            throw new RuntimeException('Réponse de connexion Google invalide.');
        }
        // Identity is read over authenticated HTTPS; unverified ID-token claims are never used.
        $profile = ($this->transport)('GET', 'https://openidconnect.googleapis.com/v1/userinfo', [], $tokens['access_token']);
        if (!is_string($profile['sub'] ?? null) || $profile['sub'] === '') {
            throw new RuntimeException('Profil Google invalide.');
        }
        return [
            'id' => $profile['sub'],
            'name' => is_string($profile['name'] ?? null) ? $profile['name'] : '',
            'email' => is_string($profile['email'] ?? null) ? $profile['email'] : '',
            'email_verified' => ($profile['email_verified'] ?? false) === true,
        ];
    }

    public static function login(array &$session, array $user, int $now): void
    {
        $session = ['user' => $user, 'authenticated_at' => $now, 'csrf' => bin2hex(random_bytes(32))];
    }

    public static function user(array &$session, int $now): ?array
    {
        if (isset($session['user']) && ($now - ($session['authenticated_at'] ?? 0) >= 28800
            || $now < ($session['authenticated_at'] ?? 0))) {
            $session = [];
        }
        return $session['user'] ?? null;
    }

    public static function logout(array &$session, mixed $csrf): void
    {
        if (!is_string($csrf) || !isset($session['csrf']) || !hash_equals($session['csrf'], $csrf)) {
            throw new RuntimeException('Requête de déconnexion invalide.');
        }
        $session = [];
    }
}

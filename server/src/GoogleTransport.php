<?php
declare(strict_types=1);
namespace Ketchup;

final class GoogleTransport
{
    public function __invoke(string $method, string $url, array $fields, ?string $token): array
    {
        $handle = curl_init($url);
        $headers = ['Accept: application/json'];
        if ($token !== null) {
            if (preg_match('/[\r\n]/', $token)) {
                throw new \RuntimeException('Réponse de connexion Google invalide.');
            }
            $headers[] = 'Authorization: Bearer ' . $token;
        }
        curl_setopt_array($handle, [CURLOPT_RETURNTRANSFER => true, CURLOPT_CONNECTTIMEOUT => 5,
            CURLOPT_TIMEOUT => 15, CURLOPT_FOLLOWLOCATION => false, CURLOPT_PROTOCOLS => CURLPROTO_HTTPS,
            CURLOPT_HTTPHEADER => $headers]);
        if ($method === 'POST') {
            curl_setopt($handle, CURLOPT_POST, true);
            curl_setopt($handle, CURLOPT_POSTFIELDS, http_build_query($fields));
        }
        $body = curl_exec($handle);
        $status = curl_getinfo($handle, CURLINFO_RESPONSE_CODE);
        if (!is_string($body) || $status !== 200) {
            throw new \RuntimeException('Google est indisponible ou a refusé la connexion. Réessayez.');
        }
        try {
            $data = json_decode($body, true, 32, JSON_THROW_ON_ERROR);
        } catch (\JsonException) {
            throw new \RuntimeException('Réponse de connexion Google invalide.');
        }
        if (!is_array($data) || isset($data['error'])) {
            throw new \RuntimeException('Réponse de connexion Google invalide.');
        }
        return $data;
    }
}

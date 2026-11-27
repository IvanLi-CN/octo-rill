#!/usr/bin/env python3
"""Validate PostgreSQL authentication extension routes against a throwaway database.

Covers passkey option/verify ceremonies (no real authenticator), GitHub connect
redirect state, LinuxDo login/callback redirects, and persistence assertions.
"""
import argparse
import base64
import http.client
import json
import os
import pathlib
import re
import subprocess

import tempfile
import time
import urllib.parse
from pg_seed_common import psql
from urllib.parse import urlsplit, urlunsplit


REDIRECT = 303


def call(base, method, path, cookie=None, payload=None):
    """Issue one HTTP request without following redirects."""
    split = urlsplit(base)
    connection = http.client.HTTPConnection(split.hostname, split.port, timeout=10)
    headers = {}
    body = None
    if payload is not None:
        body = json.dumps(payload).encode()
        headers['Content-Type'] = 'application/json'
    if cookie:
        headers['Cookie'] = cookie
    try:
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        raw = response.read()
        result = (
            response.status,
            {key.lower(): value for key, value in response.getheaders()},
            raw,
        )
    finally:
        connection.close()
    return result


def json_body(raw):
    return json.loads(raw.decode() or '{}')


def error_code(raw):
    body = json_body(raw)
    assert body.get('ok') is False, body
    return body['error']['code']


def set_cookie(headers):
    value = headers.get('set-cookie')
    assert value, 'expected a session cookie in the response'
    return value.split(';', 1)[0]


def session_flags(sql, url, cookie, tokens):
    """Report which literal tokens are persisted in the session record.

    Values are stored as MessagePack, so raw marker substrings are checked on a
    dedicated session whose only contents are the keys under test.
    """
    session_id = cookie.split('=', 1)[1]
    # tower-sessions cookie values are base64url, not hex.
    assert re.fullmatch(r'[A-Za-z0-9_-]{16,64}', session_id), session_id
    # MessagePack payloads contain NUL bytes, so compare hex-encoded markers.
    exprs = ','.join(
        f"position('{token.encode().hex()}' in encode(data, 'hex')) > 0" for token in tokens
    )
    row = sql(
        f"SELECT {exprs} FROM public.tower_sessions WHERE id = '{session_id}'", url
    )
    return [value == 't' for value in row.split('|')]


def redirect_target(headers):
    value = headers.get('location')
    assert value, 'expected a redirect location'
    return value


def wait_ready(base, process, log):
    for _ in range(150):
        try:
            call(base, 'GET', '/health')
            return
        except Exception:
            if process.poll() is not None:
                log.seek(0)
                raise RuntimeError(log.read()[-5000:])
            time.sleep(0.1)
    raise RuntimeError('validation server did not become ready')


def stop(process):
    if process is None:
        return
    process.send_signal(2)
    try:
        process.wait(timeout=20)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True, help='psql superuser URL for disposable databases')
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3031)
    args = parser.parse_args()

    name = 'octo_rill_auth_ext_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, db=args.admin_url):
        return psql(db, query)

    binary = (args.binary_dir / 'octo-rill').resolve()
    helper = (args.binary_dir / 'octo-rill-validation-session').resolve()
    for required in (binary, helper):
        if not required.exists():
            raise RuntimeError('missing validation binary: ' + str(required))

    base_env = dict(
        os.environ,
        DATABASE_URL=url,
        OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(),
        GITHUB_CLIENT_ID='validation',
        GITHUB_CLIENT_SECRET='validation',
        GITHUB_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{args.port}/auth/github/callback',
        OCTORILL_BIND_ADDR=f'127.0.0.1:{args.port}',
        OCTORILL_PUBLIC_BASE_URL=f'http://127.0.0.1:{args.port}',
        OCTORILL_PG_WORKER_ENABLED='0', OCTORILL_PG_SCHEDULER_ENABLED='0',
        # Deterministic OAuth failure: the authorize/token endpoints are compiled
        # into the client, so point the process HTTP client at a closed proxy.
        HTTP_PROXY='http://127.0.0.1:1',
        HTTPS_PROXY='http://127.0.0.1:1',
        ALL_PROXY='http://127.0.0.1:1',
        NO_PROXY='',
        no_proxy='',
    )
    for inherited in (
        'OCTORILL_PG_BOOTSTRAP_ONLY',
        'LINUXDO_CLIENT_ID',
        'LINUXDO_CLIENT_SECRET',
        'LINUXDO_OAUTH_REDIRECT_URL',
    ):
        base_env.pop(inherited, None)
    linuxdo_env = dict(
        base_env,
        LINUXDO_CLIENT_ID='linuxdo-validation',
        LINUXDO_CLIENT_SECRET='linuxdo-secret',
        LINUXDO_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{args.port}/auth/linuxdo/callback',
    )

    processes = []
    try:
        sql('CREATE DATABASE ' + name)
        bootstrap_env = dict(linuxdo_env, OCTORILL_PG_BOOTSTRAP_ONLY='1')
        subprocess.run([binary], env=bootstrap_env, check=True, stdout=subprocess.DEVNULL)
        sql(
            "INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin) "
            "VALUES('seed-auth-user',201,'seed-auth','2026','2026',0),"
            "('seed-nogithub',202,'seed-nogithub','2026','2026',0);"
            "INSERT INTO public.github_connections("
            "id,user_id,github_user_id,login,name,avatar_url,email,"
            "access_token_ciphertext,access_token_nonce,scopes,linked_at,updated_at) "
            "VALUES('seed-conn','seed-auth-user',201,'seed-auth','Seed Auth',NULL,NULL,"
            "decode('00','hex'),decode('00','hex'),'read:user','2026','2026');",
            url,
        )

        base = f'http://127.0.0.1:{args.port}'

        with tempfile.TemporaryDirectory(prefix='octo-rill-auth-ext-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-auth-user', 'seed-nogithub'):
                cookie_file = tmp / user
                subprocess.run(
                    [str(helper), '--postgres-url', url, '--user-id', user,
                     '--cookie-file', str(cookie_file), '--isolated-validation-copy'],
                    check=True, stdout=subprocess.DEVNULL,
                )
                cookies[user] = cookie_file.read_text().strip()
            # A pristine session for the GitHub connect round trip: its record
            # only carries user_id plus the OAuth keys written by the handler.
            connect_cookie_file = tmp / 'seed-auth-connect'
            subprocess.run(
                [str(helper), '--postgres-url', url, '--user-id', 'seed-auth-user',
                 '--cookie-file', str(connect_cookie_file), '--isolated-validation-copy'],
                check=True, stdout=subprocess.DEVNULL,
            )
            connect_session_cookie = connect_cookie_file.read_text().strip()

            log = (tmp / 'server.log').open('w+')
            process = subprocess.Popen([binary], env=linuxdo_env, stdout=log, stderr=subprocess.STDOUT)
            processes.append((process, log))
            wait_ready(base, process, log)

            # --- passkey registration options: onboarding (no session) ---
            status, headers, raw = call(base, 'POST', '/api/auth/passkeys/register/options')
            assert status == 200, (status, raw)
            challenge = json_body(raw)['publicKey']
            assert challenge['challenge'] and challenge['rp']['id'] == '127.0.0.1'
            assert challenge['user']['name'].startswith('passkey-')
            assert challenge['user']['displayName'] == 'OctoRill Passkey'
            assert challenge['pubKeyCredParams'], 'expected credential algorithms'
            assert 'excludeCredentials' not in challenge
            onboarding_cookie = set_cookie(headers)

            # --- passkey registration verify: challenge came from the session ---
            invalid_registration = {
                'credential': {
                    'id': 'AAAA',
                    'rawId': 'AAAA',
                    'response': {'attestationObject': 'e30', 'clientDataJSON': 'e30'},
                    'type': 'public-key',
                    'clientExtensionResults': {},
                }
            }
            status, _, raw = call(
                base, 'POST', '/api/auth/passkeys/register/verify',
                cookie=onboarding_cookie, payload=invalid_registration,
            )
            assert status == 400 and error_code(raw) == 'passkey_registration_failed', (status, raw)
            status, _, raw = call(base, 'POST', '/api/auth/passkeys/register/verify', payload=invalid_registration)
            assert status == 400 and error_code(raw) == 'passkey_registration_missing', (status, raw)

            # --- passkey authentication options/verify ---
            status, headers, raw = call(base, 'POST', '/api/auth/passkeys/authenticate/options')
            assert status == 200, (status, raw)
            authentication = json_body(raw)['publicKey']
            assert authentication['challenge'] and authentication['rpId'] == '127.0.0.1'
            assert authentication['allowCredentials'] == []
            assert authentication['userVerification'] == 'required'
            authentication_cookie = set_cookie(headers)

            invalid_authentication = {
                'credential': {
                    'id': 'AAAA',
                    'rawId': 'AAAA',
                    'response': {
                        'authenticatorData': 'AAAA',
                        'clientDataJSON': 'e30',
                        'signature': 'AAAA',
                        'userHandle': 'AAAAAAAAAAAAAAAAAAAAAA',
                    },
                    'clientExtensionResults': {},
                    'type': 'public-key',
                }
            }
            status, _, raw = call(
                base, 'POST', '/api/auth/passkeys/authenticate/verify',
                cookie=authentication_cookie, payload=invalid_authentication,
            )
            assert status == 401 and error_code(raw) == 'passkey_user_not_found', (status, raw)
            status, _, raw = call(
                base, 'POST', '/api/auth/passkeys/authenticate/verify', payload=invalid_authentication,
            )
            assert status == 400 and error_code(raw) == 'passkey_authentication_missing', (status, raw)

            # --- passkey options for authenticated users ---
            status, _, raw = call(
                base, 'POST', '/api/auth/passkeys/register/options', cookie=cookies['seed-auth-user'],
            )
            assert status == 200, (status, raw)
            authenticated_challenge = json_body(raw)['publicKey']
            assert authenticated_challenge['user']['name'] == 'seed-auth'
            assert authenticated_challenge['user']['displayName'] == 'seed-auth'
            handle_bytes = base64.urlsafe_b64decode(authenticated_challenge['user']['id'] + '==')
            assert len(handle_bytes) == 16, 'passkey user handle must encode a UUID'
            assert 'excludeCredentials' not in authenticated_challenge
            status, _, raw = call(
                base, 'POST', '/api/auth/passkeys/register/options', cookie=cookies['seed-nogithub'],
            )
            assert status == 409 and error_code(raw) == 'passkey_github_required', (status, raw)
            status, _, raw = call(base, 'GET', '/api/me/passkeys', cookie=cookies['seed-auth-user'])
            assert status == 200 and json_body(raw)['items'] == [], (status, raw)

            # --- github connect ---
            status, _, raw = call(base, 'GET', '/auth/github/connect')
            assert status == 401 and error_code(raw) == 'unauthorized', (status, raw)
            status, headers, _ = call(
                base, 'GET', '/auth/github/connect', cookie=connect_session_cookie,
            )
            assert status == REDIRECT, status
            location = urlsplit(redirect_target(headers))
            assert location.scheme == 'https' and location.hostname == 'github.com'
            assert location.path == '/login/oauth/authorize'
            query = urllib.parse.parse_qs(location.query)
            assert query['client_id'] == ['validation']
            scopes = set(query['scope'][0].split())
            assert scopes == {'read:user', 'user:email', 'notifications', 'public_repo'}
            assert query['redirect_uri'] == [f'http://127.0.0.1:{args.port}/auth/github/callback']
            connect_state = query['state'][0]
            assert connect_state
            connect_cookie = set_cookie(headers)
            assert connect_cookie == connect_session_cookie
            assert session_flags(
                sql, url, connect_cookie,
                ['user_id', 'oauth_state', 'github_oauth_mode', 'connect', 'login'],
            ) == [True, True, True, True, False], 'connect mode must be stored in the session'

            # Connect-mode callback: a mismatched state is rejected before any
            # key is consumed, so the stored mode survives untouched.
            status, _, raw = call(
                base, 'GET', '/auth/github/callback?code=invalid&state=forged',
                cookie=connect_cookie,
            )
            assert status == 400 and error_code(raw) == 'bad_request', (status, raw)
            assert session_flags(
                sql, url, connect_cookie,
                ['user_id', 'oauth_state', 'github_oauth_mode', 'connect', 'login'],
            ) == [True, True, True, True, False], 'state mismatch must not consume oauth keys'

            # With the issued state the callback consumes the keys in memory and
            # attempts the token exchange, which fails against the closed proxy.
            status, _, raw = call(
                base,
                'GET',
                '/auth/github/callback?code=invalid&state=' + urllib.parse.quote(connect_state),
                cookie=connect_cookie,
            )
            assert status == 500 and error_code(raw) == 'internal_error', (status, raw)
            # tower-sessions skips persisting on 5xx responses (legacy runtime
            # shares this layer), so the exchange failure leaves the stored
            # connect keys in place. The success path cannot be exercised
            # without a live github.com token endpoint.
            assert session_flags(
                sql, url, connect_cookie,
                ['oauth_state', 'github_oauth_mode', 'connect'],
            ) == [True, True, True], 'server errors must not persist session mutations'

            # --- github login stores the login mode ---
            status, headers, _ = call(base, 'GET', '/auth/github/login')
            assert status == REDIRECT, status
            login_location = urlsplit(redirect_target(headers))
            assert login_location.hostname == 'github.com'
            login_state = urllib.parse.parse_qs(login_location.query)['state'][0]
            login_cookie = set_cookie(headers)
            assert session_flags(
                sql, url, login_cookie,
                ['oauth_state', 'github_oauth_mode', 'login', 'connect'],
            ) == [True, True, True, False], 'login mode must be stored in the session'
            status, _, raw = call(
                base, 'GET', '/auth/github/callback?code=invalid&state=forged', cookie=login_cookie,
            )
            assert status == 400 and error_code(raw) == 'bad_request', (status, raw)
            assert login_state

            # --- linuxdo login/callback (configured) ---
            status, headers, _ = call(base, 'GET', '/auth/linuxdo/login')
            assert status == REDIRECT, status
            location = urlsplit(redirect_target(headers))
            assert location.scheme == 'https' and location.hostname == 'connect.linux.do'
            assert location.path == '/oauth2/authorize'
            query = urllib.parse.parse_qs(location.query)
            assert query['client_id'] == ['linuxdo-validation']
            assert query['scope'] == ['user']
            assert query['redirect_uri'] == [f'http://127.0.0.1:{args.port}/auth/linuxdo/callback']
            assert query['response_type'] == ['code']
            assert set_cookie(headers).startswith('octo_rill_sid=')

            status, headers, raw = call(base, 'GET', '/auth/linuxdo/callback?code=abc&state=forged')
            assert status == REDIRECT, (status, raw)
            target = urlsplit(redirect_target(headers))
            assert target.path == '/bind/github'
            assert urllib.parse.parse_qs(target.query)['linuxdo'] == ['state_mismatch']
            status, headers, _ = call(
                base, 'GET', '/auth/linuxdo/callback?code=abc&state=forged',
                cookie=cookies['seed-auth-user'],
            )
            assert status == REDIRECT, status
            target = urlsplit(redirect_target(headers))
            assert target.path == '/settings'
            assert urllib.parse.parse_qs(target.query)['linuxdo'] == ['state_mismatch']
            status, _, _ = call(base, 'GET', '/auth/linuxdo/callback?code=abc')
            assert status == 400, status

            # --- linuxdo unconfigured behavior on a second instance ---
            second_port = args.port + 1
            second_env = dict(
                base_env,
                OCTORILL_BIND_ADDR=f'127.0.0.1:{second_port}',
                OCTORILL_PUBLIC_BASE_URL=f'http://127.0.0.1:{second_port}',
                GITHUB_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{second_port}/auth/github/callback',
            )
            second_log = (tmp / 'server-unconfigured.log').open('w+')
            second = subprocess.Popen([binary], env=second_env, stdout=second_log, stderr=subprocess.STDOUT)
            processes.append((second, second_log))
            second_base = f'http://127.0.0.1:{second_port}'
            wait_ready(second_base, second, second_log)
            status, _, raw = call(second_base, 'GET', '/auth/linuxdo/login')
            assert status == 503 and error_code(raw) == 'linuxdo_oauth_not_configured', (status, raw)
            status, headers, _ = call(second_base, 'GET', '/auth/linuxdo/callback?code=abc&state=abc')
            assert status == REDIRECT, status
            target = urlsplit(redirect_target(headers))
            assert target.path == '/bind/github'
            assert urllib.parse.parse_qs(target.query)['linuxdo'] == ['not_configured']

            # --- persistence assertions ---
            assert sql('SELECT count(*) FROM public.user_passkeys', url) == '0'
            assert sql('SELECT count(*) FROM public.linuxdo_connections', url) == '0'
            assert sql(
                "SELECT count(*) FROM public.users WHERE passkey_user_handle_uuid IS NOT NULL", url
            ) == '0'
            assert sql(
                "SELECT count(*) FROM public.users WHERE last_active_at IS NOT NULL", url
            ) == '2', 'active-user gate must touch last_active_at'
            assert sql('SELECT count(*) FROM public.github_connections', url) == '1'

            print(json.dumps({
                'passkey_options_and_rejections': 'pass',
                'authenticated_passkey_options': 'pass',
                'github_connect_redirect': 'pass',
                'linuxdo_login_and_state_rejection': 'pass',
                'linuxdo_unconfigured_behavior': 'pass',
                'persistence': 'pass',
            }))
    finally:
        for process, log in processes:
            stop(process)
            log.close()
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Seed an isolated PG18 database and verify the reaction PAT/reaction endpoints.

The GitHub upstream is a local stub: the application is started with
OCTORILL_GITHUB_API_BASE pointing at it, so no real network calls happen.
"""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess

import tempfile
import threading
import time
import urllib.error
import urllib.request
from pg_seed_common import psql
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit, urlunsplit

VALID_TOKEN = 'ghp_valid_1234567890'
UNBOUND_TOKEN = 'ghp_other_9876543210'
BROKEN_TOKEN = 'ghp_bad_token_0000'
NO_SCOPE_TOKEN = 'ghp_noscope_5555'
GRAPHQL_REVOKED_TOKEN = 'ghp_revoked_7777'
API_KEY = 'orill_ak_validation_reactions_key'
API_KEY_HASH = base64.urlsafe_b64encode(hashlib.sha256(API_KEY.encode()).digest()).rstrip(b'=').decode()
RATE_LIMITED_TOKEN = 'ghp_ratelimit_9999'

RELEASE_NODE = 'RELEASE_NODE_120'


class StubUpstream:
    def __init__(self):
        self.requests = []
        self.lock = threading.Lock()
        self.totals = {RELEASE_NODE: 1, 'RELEASE_NODE_122': 7}
        self.reacted = set()
        stub = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def _reply(self, status, payload, headers=None):
                body = json.dumps(payload).encode()
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                for key, value in (headers or {}).items():
                    self.send_header(key, value)
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                if self.path != '/user':
                    self._reply(404, {'message': 'not found'})
                    return
                token = (self.headers.get('Authorization') or '').removeprefix('Bearer ').strip()
                with stub.lock:
                    stub.requests.append(('user', token))
                if token == VALID_TOKEN:
                    self._reply(200, {'id': 101, 'login': 'seed'}, {'X-OAuth-Scopes': 'repo, read:user'})
                elif token == GRAPHQL_REVOKED_TOKEN:
                    self._reply(200, {'id': 101, 'login': 'seed'}, {'X-OAuth-Scopes': 'repo'})
                elif token == UNBOUND_TOKEN:
                    self._reply(200, {'id': 202, 'login': 'intruder'}, {'X-OAuth-Scopes': 'repo'})
                elif token == NO_SCOPE_TOKEN:
                    self._reply(200, {'id': 101, 'login': 'seed'}, {'X-OAuth-Scopes': 'read:user'})
                elif token == RATE_LIMITED_TOKEN:
                    self._reply(403, {'message': 'API rate limit exceeded'},
                                {'X-RateLimit-Remaining': '0'})
                else:
                    self._reply(401, {'message': 'Bad credentials'})

            def do_POST(self):
                if self.path != '/graphql':
                    self._reply(404, {'message': 'not found'})
                    return
                length = int(self.headers.get('Content-Length') or 0)
                payload = json.loads(self.rfile.read(length) or b'{}')
                query = payload.get('query') or ''
                variables = payload.get('variables') or {}
                token = (self.headers.get('Authorization') or '').removeprefix('Bearer ').strip()
                with stub.lock:
                    stub.requests.append(('graphql', token, query))
                if token == GRAPHQL_REVOKED_TOKEN:
                    self._reply(401, {'message': 'Bad credentials'})
                    return
                if 'addReaction' in query:
                    node_id = variables['input']['subjectId']
                    content = variables['input']['content']
                    with stub.lock:
                        stub.reacted.add((node_id, content))
                        stub.totals[node_id] = stub.totals.get(node_id, 0) + 1
                        total = stub.totals[node_id]
                    subject = {
                        'id': node_id,
                        'reactionGroups': [
                            {'content': content, 'viewerHasReacted': True,
                             'reactors': {'totalCount': total}},
                        ],
                    }
                    self._reply(200, {'data': {'addReaction': {'subject': subject}}})
                    return
                if 'removeReaction' in query:
                    node_id = variables['input']['subjectId']
                    content = variables['input']['content']
                    with stub.lock:
                        stub.reacted.discard((node_id, content))
                        stub.totals[node_id] = max(stub.totals.get(node_id, 0) - 1, 0)
                        total = stub.totals[node_id]
                    subject = {
                        'id': node_id,
                        'reactionGroups': [
                            {'content': content, 'viewerHasReacted': False,
                             'reactors': {'totalCount': total}},
                        ],
                    }
                    self._reply(200, {'data': {'removeReaction': {'subject': subject}}})
                    return
                with stub.lock:
                    nodes = [
                        {
                            'id': node_id,
                            'reactionGroups': [
                                {
                                    'content': 'THUMBS_UP',
                                    'viewerHasReacted': (node_id, 'THUMBS_UP') in stub.reacted,
                                    'reactors': {'totalCount': stub.totals.get(node_id, 0)},
                                },
                            ],
                        }
                        for node_id in variables.get('ids', [])
                    ]
                self._reply(200, {'data': {'nodes': nodes}})

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def calls(self, kind):
        with self.lock:
            return [entry for entry in self.requests if entry[0] == kind]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3023)
    args = parser.parse_args()

    name = 'octo_rill_reactions_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, database=args.admin_url):
        return psql(database, query)

    sql('CREATE DATABASE ' + name)
    stub = StubUpstream().start()
    binary = (args.binary_dir / 'octo-rill').resolve()
    helper = (args.binary_dir / 'octo-rill-validation-session').resolve()
    env = dict(
        os.environ,
        DATABASE_URL=url,
        OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(),
        GITHUB_CLIENT_ID='validation',
        GITHUB_CLIENT_SECRET='validation',
        GITHUB_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{args.port}/auth/github/callback',
        OCTORILL_BIND_ADDR=f'127.0.0.1:{args.port}',
        OCTORILL_PG_WORKER_ENABLED='0', OCTORILL_PG_SCHEDULER_ENABLED='0',
        OCTORILL_PG_BOOTSTRAP_ONLY='1',
        OCTORILL_GITHUB_API_BASE=f'http://127.0.0.1:{stub.port}',
    )
    for key in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy'):
        env.pop(key, None)
    env['NO_PROXY'] = '127.0.0.1,localhost'
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    server = None
    try:
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)
        sql(
            "INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin) VALUES "
            "('seed-user',101,'seed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0),"
            "('seed-nopat',103,'nopat','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0),"
            "('seed-disabled',104,'disabled','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0);"
            "INSERT INTO public.github_connections(id,user_id,github_user_id,login,access_token_ciphertext,access_token_nonce,scopes,linked_at,updated_at) VALUES "
            "('seed-connection','seed-user',101,'seed','\\x00','\\x00','repo','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'),"
            "('nopat-connection','seed-nopat',103,'nopat','\\x00','\\x00','repo','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
            "INSERT INTO public.user_api_keys(id,user_id,name,key_hash,key_ciphertext,key_nonce,key_prefix,masked_key,created_at) VALUES "
            "('seed-api-key','seed-user','validation','" + API_KEY_HASH + "','\\x00','\\x00','orill_ak_validation','orill_ak_validation...key','2026-01-01T00:00:00Z');"
            "DROP TABLE IF EXISTS public.reaction_pat_tokens CASCADE; CREATE TABLE public.reaction_pat_tokens("
            "user_id text PRIMARY KEY,token_ciphertext bytea NOT NULL,token_nonce bytea NOT NULL,"
            "masked_token text NOT NULL,last_check_state text NOT NULL DEFAULT 'unknown',"
            "last_check_message text,last_checked_at text,updated_at text NOT NULL,"
            "owner_github_connection_id text,owner_github_user_id bigint,owner_login text,"
            "webhook_push_allows_private_repos bigint);"
            "DROP TABLE IF EXISTS public.repo_releases CASCADE; CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY,repo_id bigint NOT NULL,"
            "node_id text,tag_name text,name text,body text,html_url text,published_at text,"
            "created_at text,updated_at text NOT NULL,is_prerelease bigint DEFAULT 0,is_draft bigint DEFAULT 0,"
            "react_plus1 bigint DEFAULT 0,react_laugh bigint DEFAULT 0,react_heart bigint DEFAULT 0,"
            "react_hooray bigint DEFAULT 0,react_rocket bigint DEFAULT 0,react_eyes bigint DEFAULT 0);"
            "DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos(user_id text NOT NULL,repo_id bigint NOT NULL,full_name text);"
            "INSERT INTO public.repo_releases(release_id,repo_id,node_id,tag_name,name,html_url,updated_at) VALUES "
            "(120,1,'" + RELEASE_NODE + "','v1.0.0','Release 120','https://github.com/seed/repo/releases/1','2026-01-01T00:00:00Z'),"
            "(121,1,NULL,'v1.0.1','Release 121','https://github.com/seed/repo/releases/2','2026-01-01T00:00:00Z'),"
            "(122,2,'RELEASE_NODE_122','v2.0.0','Release 122','https://github.com/seed/other/releases/1','2026-01-01T00:00:00Z');"
            "INSERT INTO public.user_release_visible_repos VALUES "
            "('seed-user',1,'seed/repo'),('seed-nopat',2,'seed/other');",
            url,
        )

        with tempfile.TemporaryDirectory(prefix='octo-rill-reactions-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-user', 'seed-nopat', 'seed-disabled'):
                cookie_file = tmp / (user + '.cookie')
                subprocess.run(
                    [helper, '--postgres-url', url, '--user-id', user, '--cookie-file', cookie_file,
                     '--isolated-validation-copy'],
                    check=True, stdout=subprocess.DEVNULL,
                )
                cookies[user] = cookie_file.read_text().strip()
            sql("UPDATE public.users SET is_disabled=1 WHERE id='seed-disabled'", url)

            env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
            log = (tmp / 'server.log').open('w+')
            server = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f'http://127.0.0.1:{args.port}'
            for _ in range(100):
                try:
                    opener.open(base + '/health', timeout=1).close()
                    break
                except Exception:
                    if server.poll() is not None:
                        log.seek(0)
                        raise RuntimeError('server startup failed: ' + log.read()[-4000:])
                    time.sleep(.1)
            else:
                raise RuntimeError('server startup timed out')

            def call(method, path, user=None, body=None, extra_headers=None):
                headers = {'Content-Type': 'application/json'}
                if user:
                    headers['Cookie'] = cookies[user]
                headers.update(extra_headers or {})
                data = None if body is None else json.dumps(body).encode()
                request = urllib.request.Request(base + path, data=data, headers=headers, method=method)
                try:
                    with opener.open(request, timeout=15) as response:
                        return response.status, json.load(response)
                except urllib.error.HTTPError as error:
                    raw = error.read()
                    return error.code, (json.loads(raw) if raw else None)

            def error_code(body):
                assert isinstance(body, dict) and 'error' in body, body
                return body['error']['code']

            results = {}

            # --- auth boundaries -------------------------------------------------
            code, body = call('POST', '/api/reaction-token/check', None, {'token': VALID_TOKEN})
            assert code == 401 and error_code(body) == 'unauthorized', (code, body)
            code, body = call('POST', '/api/reaction-token/check', 'seed-disabled', {'token': VALID_TOKEN})
            assert code == 403 and error_code(body) == 'account_disabled', (code, body)
            code, body = call('PUT', '/api/reaction-token', None, {'token': VALID_TOKEN})
            assert code == 401 and error_code(body) == 'unauthorized', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', None,
                              {'release_id': '120', 'content': 'plus1'})
            assert code == 401 and error_code(body) == 'unauthorized', (code, body)
            code, body = call('POST', '/api/feed/reactions/refresh', None, {'release_ids': ['120']})
            assert code == 401 and error_code(body) == 'unauthorized', (code, body)
            results['auth_boundaries'] = 'pass'

            # --- token check -----------------------------------------------------
            code, body = call('POST', '/api/reaction-token/check', 'seed-user', {'token': ''})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/reaction-token/check', 'seed-user', {'token': BROKEN_TOKEN})
            assert code == 200 and body['state'] == 'invalid' and body['message'] == 'token is invalid or expired', body
            code, body = call('POST', '/api/reaction-token/check', 'seed-user', {'token': NO_SCOPE_TOKEN})
            assert code == 200 and body['state'] == 'invalid'
            assert body['message'] == 'classic PAT needs public_repo (public) or repo (private)', body
            code, body = call('POST', '/api/reaction-token/check', 'seed-user', {'token': UNBOUND_TOKEN})
            assert code == 200 and body['state'] == 'invalid'
            assert 'not bound to the current OctoRill account' in body['message'], body
            code, body = call('POST', '/api/reaction-token/check', 'seed-user', {'token': RATE_LIMITED_TOKEN})
            assert code == 429 and error_code(body) == 'rate_limited', (code, body)
            code, body = call('POST', '/api/reaction-token/check', 'seed-user', {'token': VALID_TOKEN})
            assert code == 200 and body == {
                'state': 'valid',
                'message': 'token is valid',
                'owner': {'github_connection_id': 'seed-connection', 'github_user_id': 101, 'login': 'seed'},
            }, body
            results['reaction_token_check'] = 'pass'

            # --- token upsert ----------------------------------------------------
            code, body = call('PUT', '/api/reaction-token', 'seed-user', {'token': '   '})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('PUT', '/api/reaction-token', 'seed-user', {'token': BROKEN_TOKEN})
            assert code == 422 and error_code(body) == 'pat_invalid', (code, body)
            code, body = call('PUT', '/api/reaction-token', 'seed-user', {'token': UNBOUND_TOKEN})
            assert code == 422 and error_code(body) == 'pat_invalid'
            assert 'not bound' in body['error']['message'], body
            code, body = call('PUT', '/api/reaction-token', 'seed-user', {'token': VALID_TOKEN})
            assert code == 200 and body['configured'] is True and body['masked_token'] == 'ghp_...7890', body
            assert body['check']['state'] == 'valid' and body['check']['message'] == 'token is valid', body
            assert body['owner'] == {'github_connection_id': 'seed-connection', 'github_user_id': 101, 'login': 'seed'}, body
            stored = sql(
                "SELECT encode(token_ciphertext,'hex'),octet_length(token_nonce),masked_token,"
                "last_check_state,owner_github_connection_id,owner_github_user_id,owner_login,"
                "webhook_push_allows_private_repos FROM public.reaction_pat_tokens WHERE user_id='seed-user'",
                url,
            ).split('|')
            assert VALID_TOKEN.encode().hex() not in stored[0], 'plaintext token must never be stored'
            assert stored[1] == '12' and stored[2] == 'ghp_...7890', stored
            assert stored[3] == 'valid' and stored[4] == 'seed-connection' and stored[5] == '101', stored
            assert stored[6] == 'seed' and stored[7] == '1', stored
            code, body = call('GET', '/api/reaction-token/status', 'seed-user')
            assert code == 200 and body['configured'] is True and body['masked_token'] == 'ghp_...7890', body
            assert body['check']['state'] == 'valid' and body['owner']['login'] == 'seed', body
            results['reaction_token_upsert'] = 'pass'

            # --- toggle: validation and availability -----------------------------
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '', 'content': 'plus1'})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': 'abc', 'content': 'plus1'})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-nopat',
                              {'release_id': '122', 'content': 'plus1'})
            assert code == 403 and error_code(body) == 'pat_required', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '120', 'content': 'thumbsup'})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '130', 'content': 'plus1'})
            assert code == 404 and error_code(body) == 'not_found', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '121', 'content': 'plus1'})
            assert code == 409 and error_code(body) == 'sync_required', (code, body)

            # --- toggle: add then remove -----------------------------------------
            stub.requests.clear()
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '120', 'content': 'plus1'})
            assert code == 200 and body['release_id'] == '120', (code, body)
            assert body['reactions']['status'] == 'ready', body
            assert body['reactions']['counts']['plus1'] == 2, body
            assert body['reactions']['viewer']['plus1'] is True, body
            assert len(stub.calls('graphql')) == 2, stub.calls('graphql')
            assert 'addReaction' in stub.calls('graphql')[1][2]
            assert sql("SELECT react_plus1 FROM public.repo_releases WHERE release_id=120", url) == '2'
            assert sql(
                "SELECT last_check_state,last_check_message FROM public.reaction_pat_tokens WHERE user_id='seed-user'",
                url,
            ) == 'valid|PAT is valid'

            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '120', 'content': 'plus1'})
            assert code == 200 and body['reactions']['counts']['plus1'] == 1, (code, body)
            assert body['reactions']['viewer']['plus1'] is False, body
            assert 'removeReaction' in stub.calls('graphql')[-1][2]
            assert sql("SELECT react_plus1 FROM public.repo_releases WHERE release_id=120", url) == '1'
            results['release_reaction_toggle'] = 'pass'

            # --- refresh ---------------------------------------------------------
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-user', {'release_ids': []})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-user',
                              {'release_ids': [str(value) for value in range(1, 102)]})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-user',
                              {'release_ids': ['120', 'bogus']})
            assert code == 400 and error_code(body) == 'bad_request', (code, body)
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-user',
                              {'release_ids': ['121']})
            assert code == 200 and body == {'items': []}, body
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-user',
                              {'release_ids': ['120', '121', '130', '120']})
            assert code == 200 and len(body['items']) == 1, body
            item = body['items'][0]
            assert item['release_id'] == '120' and item['reactions']['status'] == 'ready', item
            assert item['reactions']['counts']['plus1'] == 1 and item['reactions']['viewer']['plus1'] is False, item
            assert sql("SELECT react_plus1 FROM public.repo_releases WHERE release_id=120", url) == '1'
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-nopat', {'release_ids': ['122']})
            assert code == 200 and body == {'items': []}, body
            results['feed_reaction_refresh'] = 'pass'

            # --- API key auth on business endpoints ------------------------------
            bearer = {'Authorization': f'Bearer {API_KEY}'}
            code, body = call('POST', '/api/reaction-token/check', None, {'token': VALID_TOKEN}, bearer)
            assert code == 401 and error_code(body) == 'unauthorized', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', None,
                              {'release_id': '120', 'content': 'plus1'}, bearer)
            assert code == 200 and body['reactions']['counts']['plus1'] == 2, (code, body)
            assert body['reactions']['viewer']['plus1'] is True, body
            code, body = call('POST', '/api/release/reactions/toggle', None,
                              {'release_id': '120', 'content': 'plus1'}, bearer)
            assert code == 200 and body['reactions']['counts']['plus1'] == 1, (code, body)
            assert body['reactions']['viewer']['plus1'] is False, body
            code, body = call('POST', '/api/feed/reactions/refresh', None,
                              {'release_ids': ['120']}, bearer)
            assert code == 200 and len(body['items']) == 1, (code, body)
            assert body['items'][0]['reactions']['counts']['plus1'] == 1, body
            code, body = call('POST', '/api/release/reactions/toggle', None,
                              {'release_id': '120', 'content': 'plus1'},
                              {'Authorization': 'Bearer orill_ak_unknown_key'})
            assert code == 401 and error_code(body) == 'invalid_api_key', (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', None,
                              {'release_id': '120', 'content': 'plus1'},
                              {'Authorization': 'Bearer not_an_api_key'})
            assert code == 401 and error_code(body) == 'invalid_api_key', (code, body)
            results['api_key_auth'] = 'pass'

            # --- expired / revoked PAT persistence -------------------------------
            sql(
                "UPDATE public.reaction_pat_tokens SET token_ciphertext='\\x00',"
                "token_nonce='\\x00' WHERE user_id='seed-user'",
                url,
            )
            code, body = call('POST', '/api/feed/reactions/refresh', 'seed-user', {'release_ids': ['120']})
            assert code == 422 and error_code(body) == 'pat_invalid', (code, body)
            assert sql(
                "SELECT last_check_state,last_check_message FROM public.reaction_pat_tokens WHERE user_id='seed-user'",
                url,
            ) == 'invalid|PAT is invalid or expired'

            code, body = call('PUT', '/api/reaction-token', 'seed-user', {'token': GRAPHQL_REVOKED_TOKEN})
            assert code == 200, (code, body)
            code, body = call('POST', '/api/release/reactions/toggle', 'seed-user',
                              {'release_id': '120', 'content': 'heart'})
            assert code == 422 and error_code(body) == 'pat_invalid', (code, body)
            assert sql(
                "SELECT last_check_state FROM public.reaction_pat_tokens WHERE user_id='seed-user'", url,
            ) == 'invalid'
            code, body = call('PUT', '/api/reaction-token', 'seed-user', {'token': VALID_TOKEN})
            assert code == 200, (code, body)
            results['pat_invalid_persistence'] = 'pass'

            print(json.dumps(results))
    finally:
        if server is not None:
            server.send_signal(2)
            server.wait(timeout=15)
        stub.stop()
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()

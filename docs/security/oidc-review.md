# web/ OIDC 認証セキュリティレビュー (RFC 9700 + OpenID Connect Core 1.0 基準)

> **修正 (コード/設定変更) は本レビューのスコープ外**です。本レポートは現状実装の点検
> と発見事項の文書化のみを行い、コード修正はしません。発見事項のうち対応が必要なもの
> は最後の「フォローアップタスク化候補」に整理しています。

> **更新 (2026-05-02)**: F-1〜F-9 への対応は Contract A (#11, sub-tasks #403〜#408)
> ですべて完了。各項目の対応コミットと実装手段は §1 末尾「対応状況サマリー」および
> §6 「Contract A 実装結果」を参照。F-10/F-11/F-12 (Info) は本 Contract の対応スコープ
> から除外している (詳細は §6 末尾)。

- 対象リポジトリ: `senko` (本リポジトリ)
- 対象範囲: `web/` (TanStack Start) の OIDC 認証統合まわり
- 主要ライブラリ: `start-authjs ^1.0.0`, `@auth/core ^0.41.1`, `oauth4webapi` (推移依存)
- レビュー実施日: 2026-05-01
- 参照基準:
  - **RFC 9700** *Best Current Practice for OAuth 2.0 Security* (略: BCP)
  - **OpenID Connect Core 1.0** incorporating errata set 2 (略: OIDC Core)
  - 補助: OIDC RP-Initiated Logout 1.0, RFC 7636 (PKCE), RFC 6819, RFC 6749
- 評価方針: 一般論ではなく本リポジトリの実コード/設定を起点に、`web/` のソースおよび
  `node_modules/@auth/core` `node_modules/start-authjs` の挙動を実読して評価。
- 修正のスコープ外: 本レポートではコードや設定の変更は行わない。発見事項に
  「推奨対応」を記載しているが、実装は後続タスクで扱う。

---

## 1. 重大度別サマリー

| 重大度 | 件数 | 該当 No. |
| ---- | --- | --- |
| Critical | 0 | — |
| High | 2 | F-1, F-2 |
| Medium | 5 | F-3, F-4, F-5, F-6, F-7 |
| Low | 2 | F-8, F-9 |
| Info | 3 | F-10, F-11, F-12 |
| 合計 | 12 |  |

各重大度の判定基準は以下の通り。

- **Critical**: 攻撃者が現行設定で容易に認証バイパス・トークン窃取・なりすましを行える
- **High**: 設定上の追加条件 (XSS 等) が成立すれば認証バイパス・トークン窃取に直結する
- **Medium**: 仕様上の必須/強推奨に違反、または運用ミスによりバイパス・情報漏洩が起こり得る
- **Low**: 環境依存または多層防御の欠落であり実害は限定的だが、修正コストは小さい
- **Info**: 現状問題はないが、将来の変更で誤りやすい挙動。記録目的。

発見事項一覧:

| No. | タイトル | 重大度 |
| --- | --- | --- |
| F-1 | Auth.js セッションを介してアクセストークンがブラウザ JS に露出 | High |
| F-2 | OIDC `nonce` パラメータが未設定 (id_token リプレイ攻撃に対する防御欠落) | High |
| F-3 | `state` パラメータが checks に含まれていない (多層防御の欠落) | Medium |
| F-4 | `WEB_DEV_AUTH_BYPASS` が NODE_ENV ガードなしでフェイルオープン | Medium |
| F-5 | RP-Initiated Logout (IdP 側セッション終了) が未実装 | Medium |
| F-6 | アクセストークンの失効・更新フローが存在しない | Medium |
| F-7 | Content-Security-Policy 等のセキュリティヘッダ未送出 | Medium |
| F-8 | `@auth/core` が OIDC 通信で `allowInsecureRequests` を恒常的に有効化 | Low |
| F-9 | `AUTH_URL` が HTTP の場合 secure cookie が無効化される (運用前提の明文化不足) | Low |
| F-10 | 本番ビルドに TanStack Devtools が同梱される可能性 | Info |
| F-11 | `callbackUrl` の Open Redirect は default redirect callback で抑止済み | Info |
| F-12 | セッションは JWT 戦略 (DB adapter なし) で revoke 不可の特性を持つ | Info |

### 対応状況サマリー (2026-05-02 更新)

F-1〜F-9 はすべて Contract A (#11) の sub-task で実装完了。F-10/F-11/F-12 (Info) は
本 Contract のスコープ外として保留。

| No. | 重大度 | 対応状況 | 担当タスク | コミット | 概要 |
| --- | --- | --- | --- | --- | --- |
| F-1  | High   | ✅ 対応済              | #403 (A-1) | `e0547c9` | session callback から access_token 転記を削除、BFF は `getToken()` で JWT から直読 |
| F-2  | High   | ✅ 対応済              | #403 (A-1) | `e0547c9` | `oidcProvider.checks` に `nonce` を追加 (state と同時) |
| F-3  | Medium | ✅ 対応済              | #403 (A-1) | `e0547c9` | `oidcProvider.checks` に `state` を追加 (PKCE と併用) |
| F-4  | Medium | ✅ 対応済              | #404 (A-2) | `b3310db` | `NODE_ENV !== 'production'` ガード + `assertProductionConfig()` で起動時 fail-fast |
| F-5  | Medium | ✅ 対応済              | #407 (A-5) | `ae763e1` | id_token を JWT に保存し、signout で IdP の `end_session_endpoint` にリダイレクト |
| F-6  | Medium | ✅ 対応済              | #408 (A-6) | `0a649cd` | `offline_access` + jwt callback で refresh、失敗時 `session.error='RefreshAccessTokenError'` |
| F-7  | Medium | ✅ 対応済              | #405 (A-3) | `63b0373` | アプリミドルウェアで CSP (nonce ベース) / HSTS / X-Frame-Options / Referrer-Policy / X-Content-Type-Options / Permissions-Policy を送出 |
| F-8  | Low    | ✅ 対応済              | #406 (A-4) | `0e5708c` | `assertProductionConfig()` で `AUTH_OIDC_ISSUER` の HTTPS prefix を起動時 assert |
| F-9  | Low    | ✅ 対応済              | #406 (A-4) | `0e5708c` | `assertProductionConfig()` で `AUTH_URL` の HTTPS prefix を起動時 assert |
| F-10 | Info   | ⏸ 未対応 (Info で除外) | —          | —         | TanStack Devtools の本番除外。要件発生時に別 Contract で起票 |
| F-11 | Info   | 既に抑止済 (対応不要)  | —          | —         | `callbackUrl` Open Redirect は既定の redirect callback で抑止済み |
| F-12 | Info   | ⏸ 未対応 (Info で除外) | —          | —         | JWT 戦略の revoke 不可特性。強い revoke 要件発生時に DB adapter 導入を再検討 |

---

## 2. 評価対象アーキテクチャ概観

実コードと README (`web/README.md`) より抜粋:

1. ブラウザは `/_authed` 配下の保護ルートにアクセス。
2. ルート route の `beforeLoad` (`web/src/routes/__root.tsx:36-40`) が `getSession()` を
   呼び、router context にセッションを注入。
3. `_authed` (`web/src/routes/_authed.tsx:4-12`) はセッション無しなら `/login` へリダイレクト。
4. ログインフォーム (`web/src/routes/login.tsx:84-97`) は `csrfToken` と `callbackUrl=/`
   を `/api/auth/signin/oidc` に POST。`@auth/core` が PKCE で IdP に飛ばす。
5. コールバック (`web/src/routes/api/auth/$.ts:6-15`) は `start-authjs` の handler 経由で
   `@auth/core` の OAuth callback 処理に流れる。
6. JWT コールバック (`web/src/utils/auth.ts:41-46`) で `account.access_token` を JWT に
   保存。session コールバック (`auth.ts:47-52`) で session オブジェクトに転記。
7. BFF プロキシ (`web/src/routes/api/senko/$.ts:13-69`) は `getSession()` でアクセストークン
   を取り出し、`Authorization: Bearer …` を付けて `SENKO_API_BASE_URL` に転送。
8. ブラウザの Cookie は `headers.delete('cookie')` (`api/senko/$.ts:43`) で上流に渡らない。
9. `WEB_DEV_AUTH_BYPASS=true` 時は `__root.tsx` と BFF の双方が認証チェックをスキップ。

---

## 3. In Scope 各項目の点検結果

レビュー観点 (タスク `in_scope`) ごとに該当ファイルと所見を記録する。発見事項の詳細
(該当箇所/リスク/根拠/推奨対応) は §4 に記載し、ここでは要約と対応する F-番号のみ示す。

### 3.1 OIDC プロバイダー設定 (issuer/clientId/clientSecret/scope/authorization parameters)

- 該当: `web/src/utils/auth.ts:23-35`
- 設定値はすべて `process.env.AUTH_OIDC_*` から読み込み、ハードコードなし。`scope` は
  `openid profile email` の最小構成で、`offline_access` は要求していない (= refresh_token
  非取得)。
- `wellKnown` は明示指定なし → `@auth/core` の OIDC 既定で `${issuer}/.well-known/
  openid-configuration` を discovery する (`node_modules/@auth/core/lib/actions/callback/
  oauth/callback.js:38-46`)。
- 問題: `checks` を上書きしておらず default の `["pkce"]` のみ → §3.4 (F-2, F-3)。

### 3.2 ログインフロー (`web/src/routes/login.tsx` → `/api/auth/signin/oidc`)

- 該当: `web/src/routes/login.tsx:63-99`
- 流れ:
  1. `useEffect` で `fetch('/api/auth/csrf')` し、JSON から `csrfToken` を取得して
     hidden input に格納 (`login.tsx:67-80`)。
  2. `<form action="/api/auth/signin/oidc" method="POST">` で `csrfToken` と
     `callbackUrl="/"` を POST (`login.tsx:84-97`)。
  3. `@auth/core` の signin action は double-submit cookie で CSRF 検証
     (`node_modules/@auth/core/lib/actions/callback/oauth/csrf-token.js:21-33`) し、
     PKCE/state/nonce 関連 cookie を発行して IdP へ 302。
- 評価: フロントエンド側の実装は薄く、CSRF 検証・PKCE 生成は `@auth/core` が担う
  ため挙動は仕様準拠。`callbackUrl` も `/` 固定で OK (§3.9)。
- 留意: ログインフォーム自体に CSRF cookie が必要なため、最初の GET で cookie が無い
  クライアント (例: 別タブで session を破棄した直後) が `submit` 可能になる前に
  fetch が完了する必要がある (実装上 `disabled={!csrfToken}` で抑止済み: `login.tsx:93`)。

### 3.3 認可コードコールバック処理 (start-authjs ハンドラ `/api/auth/$.ts`)

- 該当: `web/src/routes/api/auth/$.ts:1-15`
- 実装は `StartAuthJS(authConfig)` を呼ぶだけで、独自の callback ロジックは無い。
  GET/POST ともに `@auth/core` の `Auth(request, config)` に流れる
  (`node_modules/start-authjs/dist/esm/handler.js:5-15`)。
- 認可コードの検証は `oauth4webapi` 経由で id_token の署名 (JWKS) / iss / aud / exp /
  iat を検証 (`node_modules/@auth/core/lib/actions/callback/oauth/callback.js:163-167`,
  `o.processAuthorizationCodeResponse`)。
- 問題: `requireIdToken` は `isOIDCProvider(provider)` により true となるが、
  `expectedNonce` は `checks.nonce.use(...)` から来るため `checks` に `"nonce"` が
  含まれない本実装では nonce 検証は行われない (§3.4 / F-2)。

### 3.4 PKCE / state パラメータの利用状況

- 該当: `web/src/utils/auth.ts:23-35` (`checks` 未指定)
- `@auth/core` の default は `checks ?? ["pkce"]`
  (`node_modules/@auth/core/lib/utils/providers.js:52-62`)。`redirectProxyUrl` 設定時のみ
  `state` が自動付与されるが、本実装では未設定。
- 結果: **PKCE のみ有効**。state も nonce も無効 → F-2, F-3。
- 注: PKCE 自体は RFC 7636 / RFC 9700 §2.1.1 に準拠し、CSRF と code interception の
  両方を緩和する。OIDC では加えて nonce が必要 (id_token 注入対策, §4 F-2 参照)。

### 3.5 id_token / access_token の検証 (issuer/aud/署名)

- 該当: `node_modules/@auth/core/lib/actions/callback/oauth/callback.js:163-167`
- `oauth4webapi` (`o.processAuthorizationCodeResponse`) が id_token に対し以下を検証:
  - JWS 署名 (issuer の JWKS から鍵取得)
  - `iss` が discovery 取得した issuer と一致
  - `aud` に client_id を含む
  - `exp` > now, `iat` の妥当性
  - `requireIdToken: true`
- 問題:
  - `nonce` は `checks` に `"nonce"` がないと検証されない (F-2)。
  - **access_token の検証はクライアント側では行わない** (これは仕様通り。OIDC Core
    §3.1.3.8 は access_token を Bearer として扱い RP は内容を検証しない)。本 BFF は
    upstream へ Bearer をそのまま渡しているため、access_token の妥当性確認は
    senko backend (本タスクでは out_of_scope) の責任。

### 3.6 トークン保管 (JWT セッションでの access_token 露出, cookie 属性)

- 該当: `web/src/utils/auth.ts:5-21, 41-52`
- アクセストークンは:
  1. JWT 内 (`token.access_token`, `auth.ts:42-44`) に保管 → サーバ側でのみ利用可能
     (Auth.js JWT は `AUTH_SECRET` で AES-256-GCM 暗号化されるため)。
  2. Session オブジェクト (`session.access_token`, `auth.ts:48-50`) に転記 → これが
     `/api/auth/session` JSON 応答にそのまま乗る (Auth.js session action 既定動作:
     `node_modules/@auth/core/lib/actions/session.js:36-46`)。
- 結果: ブラウザ JS から `fetch('/api/auth/session')` でアクセストークンを取得可能 (F-1)。
- Cookie 属性 (`@auth/core` 既定, `node_modules/@auth/core/lib/utils/cookie.js:42-105`):
  - `(__Secure-)authjs.session-token`: `httpOnly`, `sameSite=lax`, `path=/`, `secure=AUTO`
    (URL のプロトコルから推定)
  - `__Host-authjs.csrf-token`: `httpOnly`, `sameSite=lax`, `path=/`, `secure=AUTO`
  - `(__Secure-)authjs.pkce.code_verifier`: `httpOnly`, `sameSite=lax`, `secure=AUTO`,
    `maxAge=900s`
  - `(__Secure-)authjs.state`: 同上
  - `(__Secure-)authjs.nonce`: 同上
- 評価: cookie 属性は概ね適切。ただし `sameSite=lax` は OAuth コールバックを成立させる
  ための妥協 (Strict だと外部 IdP からのリダイレクト時に cookie が送られない)。
  これは Auth.js のライブラリ既定であり、本実装に固有の問題ではない。
- 課題: `secure` 属性は **`AUTH_URL` のプロトコル依存** で、HTTP の `AUTH_URL` を本番に
  設定すると無効化される (F-9)。

### 3.7 セッション管理 (有効期限, ローテーション, 無効化)

- 該当: `node_modules/@auth/core/lib/init.js:38-76`
- 既定: `strategy: "jwt"` (DB adapter 未設定のため強制), `maxAge: 30 days`,
  `updateAge: 24h`, `generateSessionToken: crypto.randomUUID()`。
- 本実装はこれらを上書きしておらず既定のまま。
- ローテーション: session が利用されるたびに `updateAge` を超えていれば新しい JWT を
  発行・cookie 更新 (`node_modules/@auth/core/lib/actions/session.js:42-54`)。
- 無効化: JWT 戦略のため **サーバ側で個別セッションを失効させる手段がない** (F-12)。
  ログアウト時にローカルの cookie をクリアするのみ (F-5)。

### 3.8 CSRF 保護 (`/api/auth/csrf`, signin POST)

- 該当: `web/src/routes/login.tsx:67-97` + `node_modules/@auth/core/lib/actions/
  callback/oauth/csrf-token.js`
- 二重送信 cookie + HMAC 検証パターン:
  - cookie: `__Host-authjs.csrf-token` の値は `{token}|{HMAC(token, secret)}` 形式
  - signin POST の form body に同じ `csrfToken` を含める
  - サーバ側で cookie の HMAC を再計算し token を検証 (`csrf-token.js:21-29`)
  - 一致した上で POST body の `csrfToken` が cookie 内 token と一致したら OK
    (`csrf-token.js:30`)
- 評価: OWASP Double-Submit Cookie + Signed Token のハイブリッドで適切
  (`csrf-token.js:14-15` のコメント参照)。`__Host-` プレフィクスにより同一オリジン
  保護も付く。
- 注意: フロントエンド (`login.tsx`) は `csrfToken` を JS から fetch して hidden input
  にコピーしている。これは `__Host-authjs.csrf-token` cookie が `httpOnly` なため
  必要な設計だが、`/api/auth/csrf` 応答が JSON として返すこと自体に問題はない (token
  自体は秘匿情報ではなく、cookie 内の HMAC こそが認証要素)。

### 3.9 callbackUrl/redirect の Open Redirect リスク

- 該当: `web/src/routes/login.tsx:92` (`callbackUrl="/"`) +
  `node_modules/@auth/core/lib/utils/callback-url.js` +
  `node_modules/@auth/core/lib/init.js:13-19` (default redirect callback)
- 既定 redirect callback は次のいずれかでない URL を `baseUrl` に丸める:
  1. `/` で始まる相対 URL → `${baseUrl}${url}` を返す
  2. `new URL(url).origin === baseUrl` → そのまま返す
  3. それ以外 → `baseUrl` を返す
- 本実装は redirect callback を上書きしておらず default のまま。
- 評価: **Open Redirect は現状抑止済み** (F-11, Info)。ただし本ファイル
  (`auth.ts`) で `callbacks.redirect` を将来的に上書きする際にこの保護を破る可能性が
  あるため、レビュー時の注意点として記録する。

### 3.10 ログアウトフロー / セッション終了

- 該当: `node_modules/@auth/core/lib/actions/signout.js:8-32` (フロントエンドに固有
  ロジックなし。README §Authentication & BFF の手順 6 で `/api/auth/signout` を案内)
- Auth.js の signout action は:
  1. session cookie をクリア
  2. `events.signOut` を発火
- IdP 側の `end_session_endpoint` 呼び出し (RP-Initiated Logout) は **行わない**。
- 結果: ローカル Auth.js cookie のみ失効し、IdP 側のセッションは生存 (F-5)。
- 注: Auth.js は POST signout を期待。GET 経由は確認画面を返す (XSS 経由で勝手に
  ログアウトさせられないようにするための CSRF 保護)。本実装には GET signout を
  自動 POST する独自 UI は無いため、ユーザは Auth.js 既定の確認画面を経由する。

### 3.11 環境変数 (`AUTH_SECRET`, `AUTH_OIDC_*`) の取り扱い

- 該当: `web/.env.example`, `web/src/utils/auth.ts:27-38`,
  `web/src/routes/__root.tsx:23`, `web/src/routes/api/senko/$.ts:14, 29`
- すべて `process.env.*` 経由で読み込み、`.env*` は `web/.gitignore` で除外
  (`web/.gitignore:3, 6-7`)。`.env.example` には placeholder のみ (`AUTH_SECRET=replace-
  with-32+-byte-random-string` 等)。
- `AUTH_SECRET` の長さ要求: README で `≥ 32 bytes` と明示 (`web/README.md:110`)。
  Auth.js は短い secret を許容するが、JWT 暗号化強度のため `openssl rand -base64 32`
  と推奨 (`web/.env.example:5-6`)。
- `AUTH_TRUST_HOST` / `AUTH_URL` の挙動: `AUTH_URL` が設定されていれば `trustHost` が
  自動 true (`node_modules/@auth/core/lib/utils/env.js:40-44`)。`.env.example` に
  `AUTH_URL` を必須として記載しているため通常は問題なし。
- 課題: `AUTH_URL` が HTTP だと secure cookie がオフ (F-9)。`AUTH_OIDC_ISSUER` が
  HTTP でも通信が成立してしまう (F-8)。

### 3.12 Cookie の Secure/HttpOnly/SameSite 設定

- §3.6 と重複。要約: `httpOnly: true`, `sameSite: 'lax'` は全 cookie で固定。
  `secure` は `AUTH_URL` プロトコル (HTTPS なら true)。`__Host-` / `__Secure-` プレフィクス
  も自動付与される。
- 評価: 既定動作で OWASP Cookie best practices をほぼ満たす。`SameSite=Strict` ではなく
  `Lax` だが、これは OAuth リダイレクトを成立させるための仕様上の妥協。

---

## 4. 発見事項詳細

### F-1. Auth.js セッションを介してアクセストークンがブラウザ JS に露出 (High)

- **該当箇所**: `web/src/utils/auth.ts:47-52`
  ```ts
  async session({ session, token }) {
    if (token.access_token) {
      session.access_token = token.access_token
    }
    return session
  },
  ```
  および `web/src/utils/auth.ts:11-15` の `Session` 型拡張。
- **リスク説明**: Auth.js の `/api/auth/session` JSON エンドポイントは session callback の
  返り値をそのままレスポンスに載せる (`node_modules/@auth/core/lib/actions/session.js:36-
  46`)。よって任意のブラウザ JS から `fetch('/api/auth/session').then(r => r.json())` で
  IdP 発行の access_token を取得できる。
  - これにより XSS が成立した場合、アクセストークンを攻撃者サーバへ送信され、攻撃者は
    senko backend の API を直接呼び出せる (BFF の意義の喪失)。
  - また、Web Vitals/解析タグ・誤って混入したサードパーティスクリプトが access_token を
    取得・送信するリスクもある。
  - README §Authentication & BFF (`web/README.md:118-153`) は「ブラウザ cookie を上流に
    漏らさない」など BFF の意図を明確に述べているが、access_token を session に転記する
    本実装はその意図と矛盾している。
- **重大度**: High (XSS 等の追加条件で即トークン窃取に至る)。
- **根拠**:
  - **OIDC Core §16.18** *Token Substitution Attacks* および §16.21 *TLS Requirements*
    で「id_token / access_token の取り扱いは保護せよ」と要求。
  - **RFC 9700 §2.6** *Browser-based Apps* (および §4.3 *Public clients in browsers* /
    BCP for Browser-Based Apps draft の系譜) で **「access_token は機密情報として扱い、
    可能な限りブラウザに渡さない」「BFF パターンが推奨」** と明記。
  - **OWASP ASVS V8.3** "Tokens are not stored in client-side storage and are protected
    against XSS"。
- **推奨対応**:
  - session callback で `session.access_token` への転記をやめる。
  - BFF (`web/src/routes/api/senko/$.ts`) はサーバ側で `getSession()` から JWT 内の
    `access_token` を取り出しており、ブラウザに渡す必要は無い。
  - Auth.js 型拡張 (`auth.ts:11-15, 17-21`) も削除し、`AuthSession.access_token` を
    UI 層から参照する経路を断つ。

- **対応**: ✅ task #403 (A-1, Contract #11), commit `e0547c9` — session callback と
  `Session.access_token` 型拡張を削除。BFF は `getToken()` で JWT から直接 access_token を
  取得し、`/api/auth/session` JSON は `{user, expires}` のみ返す。

### F-2. OIDC `nonce` パラメータが未設定 (High)

- **該当箇所**: `web/src/utils/auth.ts:23-35` の `oidcProvider` 定義 (`checks` を未指定)
  ```ts
  const oidcProvider: OIDCConfig<Profile> = {
    id: 'oidc',
    name: 'OIDC',
    type: 'oidc',
    issuer: process.env.AUTH_OIDC_ISSUER,
    clientId: process.env.AUTH_OIDC_CLIENT_ID,
    clientSecret: process.env.AUTH_OIDC_CLIENT_SECRET,
    authorization: {
      params: {
        scope: 'openid profile email',
      },
    },
  }
  ```
  ライブラリ既定: `node_modules/@auth/core/lib/utils/providers.js:52` で
  `const checks = c.checks ?? ["pkce"]` のため、`nonce` は **無効**。
- **リスク説明**: nonce が無効だと、`@auth/core` のコールバック処理 (`node_modules/
  @auth/core/lib/actions/callback/oauth/callback.js:163-167`) で `expectedNonce`
  パラメータが `undefined` となり、`oauth4webapi` の `processAuthorizationCodeResponse`
  は id_token の `nonce` claim を検証しない。結果、攻撃者がトークンエンドポイントへの
  応答 (もしくは別のセッションで取得した id_token) をブラウザに注入する形での **id_token
  リプレイ/置換攻撃** に対する防御が無くなる。
  - 具体的には RFC 9700 §4.5.3.4 (id_token replay), OIDC Core §3.1.2.1, §15.5.2 で扱う
    脅威。
- **重大度**: High (OIDC Core で nonce は **REQUIRED** に分類)。
- **根拠**:
  - **OIDC Core §3.1.2.1** (Authentication Request) — `nonce` パラメータについて
    *"Use of the nonce Claim is REQUIRED for some flows where it is needed to mitigate
    replay attacks."* (Implicit/Hybrid フローでは MUST、Authorization Code フローでも
    強推奨)。
  - **OIDC Core §15.5.2** *Nonce Implementation Notes*。
  - **RFC 9700 §4.5.3.4** *Mix-up and Honest-Client Attacks via Token Replay* の対策の
    一つとして `nonce` または `state` のいずれかを必ず使うべきと明記。
- **推奨対応**: `oidcProvider` に `checks: ['pkce', 'state', 'nonce']` を追加する。
  Auth.js は `nonce` cookie / state cookie を自動発行する (`node_modules/@auth/core/lib/
  actions/callback/oauth/checks.js`)。

- **対応**: ✅ task #403 (A-1, Contract #11), commit `e0547c9` — `oidcProvider.checks` に
  `nonce` を追加 (`['pkce', 'state', 'nonce']`)。Auth.js が nonce cookie を自動発行し、
  callback 処理で id_token の `nonce` claim 検証が有効化された。

### F-3. `state` パラメータが checks に含まれていない (Medium)

- **該当箇所**: F-2 と同じ。`auth.ts:23-35` で `checks` 未指定 → default `["pkce"]` のみ。
- **リスク説明**: PKCE は RFC 9700 §2.1.1 で「state と等価な CSRF 保護」と認められて
  おり、PKCE があれば state は必須ではない。とはいえ:
  - state は OIDC Core §3.1.2.1 / §15.5.2 で「攻撃緩和と RP 側の状態保持に有用」と
    推奨されている。
  - 多層防御として PKCE + state の併用が RFC 9700 §2.1 / §4.7 のサンプル実装でも示される。
  - `start-authjs` の `redirectProxyUrl` を将来導入した場合は state が自動有効化される
    が、現状そのフラグも無いため state はまったく送られていない。
- **重大度**: Medium (PKCE があるため即時の脅威は低いが、ベストプラクティスからの逸脱)。
- **根拠**:
  - **OIDC Core §3.1.2.1**, **§15.5.2** (state 推奨)。
  - **RFC 9700 §2.1** *Protecting Redirect-Based Flows* - PKCE と state を併用した例。
  - **RFC 6749 §10.12** *Cross-Site Request Forgery*。
- **推奨対応**: F-2 の対応と合わせて `checks: ['pkce', 'state', 'nonce']` を設定。

- **対応**: ✅ task #403 (A-1, Contract #11), commit `e0547c9` — F-2 と同じ修正に含まれる
  (`oidcProvider.checks` に `state` を追加)。PKCE と併用する多層防御を確立。

### F-4. `WEB_DEV_AUTH_BYPASS` が NODE_ENV ガードなしでフェイルオープン (Medium)

- **該当箇所**:
  - `web/src/routes/__root.tsx:22-32`
    ```ts
    const fetchSession = createServerFn({ method: 'GET' }).handler(async () => {
      if (process.env.WEB_DEV_AUTH_BYPASS === 'true') {
        const expires = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString()
        return {
          user: { name: 'dev-user', email: 'dev@localhost' },
          expires,
        } satisfies AuthSession
      }
      ...
    })
    ```
  - `web/src/routes/api/senko/$.ts:13-27`
    ```ts
    async function proxy({ request }: { request: Request }): Promise<Response> {
      const devBypass = process.env.WEB_DEV_AUTH_BYPASS === 'true'

      let accessToken: string | undefined
      if (!devBypass) {
        const session = await getSession(request, authConfig)
        accessToken = session?.access_token

        if (!accessToken) {
          return new Response(JSON.stringify({ error: 'unauthorized' }), {
            status: 401,
            headers: { 'content-type': 'application/json' },
          })
        }
      }
      ...
    }
    ```
- **リスク説明**: `WEB_DEV_AUTH_BYPASS=true` を本番環境変数に誤って混入させると、
  - ルート route がフェイクの `dev-user` セッションを返し、`_authed` 以下が誰でも閲覧可能
  - BFF プロキシが session 検査を完全にスキップし、Authorization ヘッダなしで
    `SENKO_API_BASE_URL` に転送する (senko backend が anonymous を許可していると
    全 API が無認証で公開)
  - すなわち **完全な認証バイパス**。
  README (`web/README.md:116`, `.env.example:30-37`) には「Local development only — never
  enable in production」と明記されているが、コードには `NODE_ENV !== 'production'` 等の
  実行時ガードが無いため、運用ミスが致命的になる。
- **重大度**: Medium (誤設定がトリガで Critical 相当の影響、ただしあくまで運用ミスの
  範疇)。
- **根拠**:
  - **RFC 9700 §3** *Authentication and Authorization Best Practices* — 開発専用機構を
    本番から排除する原則。
  - **RFC 6819 §5.2.4.4** *Sensitive Data in Code* に類する設定値の保護原則。
  - OWASP ASVS V14.3 *Configuration: Verify that the application is hardened against
    debug features*。
- **推奨対応**:
  - 起動時または最初の参照時に
    `if (process.env.NODE_ENV === 'production' && process.env.WEB_DEV_AUTH_BYPASS === 'true')
    throw new Error(...)` でプロセスを落とす。
  - もしくは bypass を `process.env.NODE_ENV !== 'production' && WEB_DEV_AUTH_BYPASS === 'true'`
    のように本番では決して有効化しないガードに修正。
  - README の警告文をコード内コメントにも転記し、レビュー時の見落としを抑止。

- **対応**: ✅ task #404 (A-2, Contract #11), commit `b3310db` — `__root.tsx` の
  `fetchSession` および `api/senko/$.ts` の BFF で `process.env.NODE_ENV !== 'production'`
  ガードを追加。加えて新設の `web/src/utils/assert-prod.ts` の `assertProductionConfig()`
  が `web/src/server-entry.ts` から呼ばれ、`(NODE_ENV=production && WEB_DEV_AUTH_BYPASS=true)`
  で起動時に fail-fast。

### F-5. RP-Initiated Logout (IdP 側セッション終了) が未実装 (Medium)

- **該当箇所**: `web/src/routes/api/auth/$.ts:6-15` (Auth.js の signout 既定動作のみ)。
  Auth.js signout の挙動: `node_modules/@auth/core/lib/actions/signout.js:8-32` (cookie
  クリアのみ)。
- **リスク説明**:
  - 現状の signout はローカル Auth.js cookie をクリアするだけで、IdP 側のセッション
    (および対応する access_token / refresh_token) は IdP 側で生存する。
  - これにより、共有端末で別ユーザがログアウト後に再度「Sign in」を押すと、IdP 側で
    既ログイン中のユーザとして即時 SSO されてしまう (再認証なし)。
  - access_token は revoke されないので、もし F-1 で漏れていれば失効まで利用可能。
- **重大度**: Medium (セッション固定/共有端末利用の文脈で問題)。
- **根拠**:
  - **OpenID Connect RP-Initiated Logout 1.0** §2 (RP-initiated logout 推奨)。
  - **OIDC Core §3.1.3.6** id_token を `id_token_hint` として `end_session_endpoint`
    に送ることでログアウトを完了する標準フロー。
  - **RFC 9700 §4.14** *Token Lifecycle*: 不要になったトークンの失効を推奨。
- **推奨対応**:
  - JWT に `id_token` を保管 (現在 `account.access_token` のみ保管) し、signOut 時に
    IdP の `end_session_endpoint` に `id_token_hint=...&post_logout_redirect_uri=...`
    でリダイレクト。
  - 加えて IdP が token revocation エンドポイント (RFC 7009) を提供している場合は
    access_token / refresh_token を revoke する。

- **対応**: ✅ task #407 (A-5, Contract #11), commit `ae763e1` — jwt callback で
  `account.id_token` を JWT に保存。`web/src/utils/security/oidc-discovery.ts` (1h positive
  / 60s negative TTL + single-flight) を新設し discovery を一元化。`routes/api/auth/$.ts`
  の POST handler が `/signout(?:/<provider>)?$` を intercept し、Auth.js POST signout 後
  に `Location` を `end_session_endpoint?id_token_hint=...&post_logout_redirect_uri=${origin}/login`
  に書き換え。id_token 不在 / discovery 失敗 / Auth.js 非3xx / Location 不在で Auth.js
  既定の redirect にフォールバック。
- **前提条件 (運用)**: IdP の RP 設定に `post_logout_redirect_uri = ${origin}/login` を
  事前登録すること (Keycloak / Auth0 / Authentik 共通)。

### F-6. アクセストークンの失効・更新フローが存在しない (Medium)

- **該当箇所**: `web/src/utils/auth.ts:41-46` (jwt callback)
  ```ts
  async jwt({ token, account }) {
    if (account?.access_token) {
      token.access_token = account.access_token
    }
    return token
  },
  ```
- **リスク説明**:
  - `account` は **初回サインイン時のみ** jwt callback に渡される (Auth.js 仕様)。以降
    の jwt 呼び出しでは `account === undefined`。本実装はこれを前提に access_token を
    一度だけ保存する。
  - そのため access_token の有効期限 (`account.expires_at` / `tokens.expires_in`) を
    保存しておらず、期限管理も refresh_token による更新も行わない。
  - 結果:
    1. 短命な access_token (例: 5 分) を発行する IdP 構成では、5 分後に BFF が 401 を
       返し続ける。`api/senko/$.ts` の 401 はクライアントに伝搬し、ユーザは突然
       `/login` にリダイレクトされる。
    2. Auth.js の session.maxAge (30 日 default) はそのままなので、cookie は生きている
       のに API は失敗するという乖離が起きる。
    3. F-1 と組み合わさると、漏洩した access_token は短期間しか使えないが、本実装は
       `offline_access` を要求していないので refresh_token が無く更新フローも無い。
  - 細かい話だが `tokens.expires_at` を保存していないため、access_token を upstream に
    送る前に「期限切れか」を判定する手立てが無い。
- **重大度**: Medium (UX / Liveness の問題と、暗黙の長期セッション)。
- **根拠**:
  - **RFC 9700 §4.13** *Refresh Tokens*。
  - **OIDC Core §11** *Offline Access*。
  - **RFC 6749 §1.5** *Refresh Token*。
- **推奨対応**:
  - jwt callback で `token.access_token`, `token.expires_at`, `token.refresh_token` を
    保存。
  - 後続呼び出しで期限切れなら IdP の token endpoint に `grant_type=refresh_token` で
    リクエストし、新しい access_token を取得して JWT に書き戻す (Auth.js docs:
    https://authjs.dev/guides/refresh-token-rotation)。
  - `scope` に `offline_access` を追加して refresh_token を取得する (IdP 側設定が必要)。
  - 失敗時は session.error を立て、フロントエンドで `/login` に誘導する。

- **対応**: ✅ task #408 (A-6, Contract #11), commit `0a649cd` — `scope` に `offline_access`
  を追加。jwt callback が `access_token / refresh_token / expires_at` を JWT に保存し、
  `expires_at - 60s` の leeway で IdP の token endpoint に `grant_type=refresh_token` を
  POST して更新。失敗時は `token.error='RefreshAccessTokenError'` を立て、session callback
  が surface する。BFF (`/api/senko/$.ts`) は 401 + `{error}` JSON、UI (`_authed.tsx`) は
  `beforeLoad` で `/login` リダイレクト。実装は `web/src/utils/auth/refresh.ts` の純関数で、
  vitest 8 ケース (success / no-rotation / HTTP 4xx / invalid JSON / missing endpoint /
  network throw / missing access_token / Basic auth header) で単体テスト済み。
  `oidc-discovery.ts` を拡張して `token_endpoint` も discover。
- **前提条件 (運用)**: IdP の RP 設定で `offline_access` scope と refresh token rotation
  を有効化すること。

### F-7. Content-Security-Policy 等のセキュリティヘッダ未送出 (Medium)

- **該当箇所**:
  - `web/src/routes/__root.tsx:41-49` (`head()` で `meta` 4 件のみ。CSP/X-Frame-
    Options/HSTS/Referrer-Policy/Permissions-Policy 等の送出なし)
  - `web/vite.config.ts:8-13` (TanStack Start プラグインの既定。独自 plugin による
    ヘッダ追加なし)
- **リスク説明**:
  - **CSP 不在**: F-1 と組み合わさると、XSS で挿入されたスクリプトがアクセストークンを
    取得して攻撃者サーバへ送信できる。CSP `connect-src 'self'` 等で外部送信を抑止すれば
    被害を縮小できる。また `script-src 'self'` で inline script の実行を抑止できる
    (なお `__root.tsx:34, 48` で `themeBootstrap` を inline script として注入している
    ため、CSP 導入時は nonce/hash 戦略が必要)。
  - **X-Frame-Options / frame-ancestors 不在**: clickjacking で signin / signout を
    勝手に発火させられる可能性 (RFC 9700 §4.16 *Clickjacking*)。
  - **Strict-Transport-Security 不在**: 本番が HTTPS でも HSTS を返さないと、初回 HTTP
    アクセスで MITM が成立しうる。
  - **Referrer-Policy 不在**: 既定で外部リンクに full URL を載せうる。OAuth コールバック
    URL に query (state, code) が乗ったまま外部に漏れる懸念。
- **重大度**: Medium (XSS/クリックジャック等の前提下で被害拡大要因)。
- **根拠**:
  - **RFC 9700 §4.16** *Clickjacking* (frame-ancestors 推奨)
  - **RFC 9700 §4.10** *Authorization Code Leakage through Referrer Headers*
    (Referrer-Policy 推奨)
  - **OWASP Secure Headers Project** (CSP, HSTS, X-Frame-Options 推奨)
- **推奨対応**:
  - TanStack Start のサーバミドルウェアで以下のヘッダを送出:
    - `Content-Security-Policy: default-src 'self'; script-src 'self' 'nonce-...'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'`
    - `Strict-Transport-Security: max-age=31536000; includeSubDomains`
    - `X-Frame-Options: DENY` (CSP frame-ancestors の旧ブラウザ向け補助)
    - `Referrer-Policy: strict-origin-when-cross-origin`
    - `X-Content-Type-Options: nosniff`
    - `Permissions-Policy: ...` (必要に応じ)
  - inline script (`themeBootstrap`) は CSP nonce で許可するか外部ファイルへ移す。

- **対応**: ✅ task #405 (A-3, Contract #11), commit `63b0373` —
  `web/src/utils/security/csp.ts` (純粋ヘルパ + Symbol-based bridge), `web/src/start.ts`
  (TanStack Start request middleware + `AsyncLocalStorage` を `import.meta.env.SSR` でガード),
  `web/src/router.tsx` (`readCurrentRequestNonce` → `ssr.nonce` 注入) を追加。送出ヘッダ:
  CSP (nonce ベース、dev は Report-Only モード)、HSTS (本番のみ)、X-Frame-Options、
  Referrer-Policy、X-Content-Type-Options、Permissions-Policy。
- **設計逸脱**: `style-src` は `'self' 'unsafe-inline'` を採用 (タスク説明の
  `'self'` から逸脱)。理由: 既存コードに `style={{...}}` が 16 箇所以上あり、本タスクの
  主眼は script の strict 化であり inline style XSS は許容範囲。
- **検証**: 新規 e2e `web/tests/e2e/specs/09-security-headers.spec.ts` で 6 ヘッダ送出と
  CSP nonce 注入を assert。`__root.tsx` は無改変 (HeadContent が `router.options.ssr.nonce`
  から自動 nonce 付与)。
- **dev 体験**: dev は `Content-Security-Policy-Report-Only` モード (Vite HMR / React
  refresh が `'unsafe-eval'` を要求するため enforcing は dev 体験を壊すので報告のみ)。

### F-8. `@auth/core` が OIDC 通信で `allowInsecureRequests` を恒常的に有効化 (Low)

- **該当箇所**:
  - `node_modules/@auth/core/lib/actions/callback/oauth/callback.js:42`
    (`o.allowInsecureRequests` を discoveryRequest に設定)
  - 同 `:113` (authorizationCodeGrantRequest)
  - 同 `:178, 191, 197` (userInfoRequest)
  - 同行群はコメントで `// TODO: move away from allowing insecure HTTP requests` と
    明示されている。
- **リスク説明**: ライブラリ側で oauth4webapi に `allowInsecureRequests: true` を渡して
  いるため、`AUTH_OIDC_ISSUER` が `http://...` の場合でも OIDC discovery / token /
  userinfo 通信が成立してしまう。本来 oauth4webapi は HTTPS を強制する設計だが、それを
  バイパスしている。
  - 本実装側 (`auth.ts`) で issuer URL のスキーム検証は無く、env で何でも受け付ける。
  - これにより内部ネットワークの平文 IdP を意図せず本番設定してしまう/MITM の余地。
- **重大度**: Low (アプリ側の env 設定運用次第。ライブラリ起因)。
- **根拠**:
  - **RFC 9700 §3** *General Authorization Server Recommendations* - "All
    communications must be confidentiality and integrity protected (TLS)"。
  - **OIDC Core §16.21** *TLS Requirements*。
- **推奨対応**:
  - `auth.ts` で `process.env.AUTH_OIDC_ISSUER` が `https://` で始まるかを **起動時に
    assert** し、HTTP の場合は本番起動を拒否する (NODE_ENV を絡めて dev は許可)。
  - `@auth/core` の更新で `allowInsecureRequests` が外れたバージョンへの追従を計画。

- **対応**: ✅ task #406 (A-4, Contract #11), commit `0e5708c` — `assertProductionConfig()`
  を拡張し、`process.env.NODE_ENV === 'production'` 時に `AUTH_OIDC_ISSUER` の `https://`
  prefix を起動時 assert (HTTP なら fail-fast)。失敗は F-4/F-9 と統合された multi-line
  throw で集約報告される。`@auth/core` 側の `allowInsecureRequests` は引き続き有効だが、
  本実装側の URL スキーム検証が網羅するため運用上の影響は閉じる。

### F-9. `AUTH_URL` が HTTP の場合 secure cookie が無効 (Low)

- **該当箇所**:
  - `node_modules/@auth/core/lib/init.js:69` の
    `cookie.defaultCookies(config.useSecureCookies ?? url.protocol === "https:")`
  - `node_modules/@auth/core/lib/utils/cookie.js:42-50` (`cookiePrefix = useSecureCookies
    ? "__Secure-" : ""`)
- **リスク説明**: `AUTH_URL` (またはホスト推定 URL) が HTTP のときは
  - `Secure` 属性が付かない → 平文 HTTP に乗ってしまう
  - `__Secure-` / `__Host-` プレフィクスが外れる → cookie が cross-site 経由でも書き換え
    可能になる
  - F-7 と組み合わさると HSTS 不在 + 平文 cookie で MITM の影響範囲が広がる。
  - dev では HTTP 必須なので OK だが、本番は必ず HTTPS にする運用前提が **コード上に
    強制されていない** ことが課題。
- **重大度**: Low (運用前提の問題)。
- **根拠**:
  - **RFC 9700 §3** TLS 要件。
  - **OWASP Cookie Best Practices** (Secure 属性、`__Host-` prefix 推奨)。
- **推奨対応**:
  - F-8 と同様、起動時に `process.env.AUTH_URL` が HTTPS で始まるかを assert する
    (本番のみ)。
  - もしくは `authConfig.useSecureCookies = true` を本番固定で渡す。

- **対応**: ✅ task #406 (A-4, Contract #11), commit `0e5708c` — F-8 と同じ修正に含まれる
  (`assertProductionConfig()` で `AUTH_URL` の HTTPS prefix 起動時 assert)。`@auth/core`
  が `secure` cookie 属性を `AUTH_URL` プロトコルから推定する仕様はそのままだが、本番で
  HTTP の `AUTH_URL` を弾くため secure cookie が無効になるケースは事実上閉じる。新規
  vitest `web/src/utils/assert-prod.test.ts` の 8 ケースで assert 動作を確認済み。

### F-10. 本番ビルドに TanStack Devtools が同梱される可能性 (Info)

- **該当箇所**: `web/src/routes/__root.tsx:7-8, 63-71`
  ```tsx
  import { TanStackRouterDevtoolsPanel } from '@tanstack/react-router-devtools'
  import { TanStackDevtools } from '@tanstack/react-devtools'
  ...
  <TanStackDevtools
    config={{ position: 'bottom-right' }}
    plugins={[
      {
        name: 'Tanstack Router',
        render: <TanStackRouterDevtoolsPanel />,
      },
    ]}
  />
  ```
  および `web/vite.config.ts:8-13` の `devtools()` プラグイン。
- **リスク説明**: Devtools は内部状態 (router context など session 含む) を可視化する
  ため、本番ビルドに含まれるとエンドユーザのブラウザで session が観察できる可能性が
  ある。`@tanstack/react-router-devtools` は通常 dev ビルドのみだが、現実装では条件分岐
  なしに常に描画されている。
  - 実際には `vite build` の minify と Tree-shaking で消える可能性もあるが、コード上
    `process.env.NODE_ENV` 条件などで明示除外していない。
- **重大度**: Info (実害がビルド設定依存であり、本レビューでは未検証)。
- **根拠**:
  - **OWASP ASVS V14.3** Debug 機能の本番排除。
- **推奨対応**:
  - `__root.tsx` 内で `import.meta.env.DEV` または `process.env.NODE_ENV !== 'production'`
    で TanStackDevtools の描画を分岐。
  - 本番ビルドのバンドルアナライズ (`vite-bundle-analyzer` 等) で devtools が含まれて
    いないことを確認する手順を追加。

### F-11. `callbackUrl` の Open Redirect は default redirect callback で抑止済み (Info)

- **該当箇所**: `web/src/routes/login.tsx:92` で `callbackUrl="/"` 固定 +
  `node_modules/@auth/core/lib/utils/callback-url.js` で
  `callbacks.redirect({ url, baseUrl })` を経由 +
  `node_modules/@auth/core/lib/init.js:13-19` の default redirect callback。
- **リスク説明**: 現状 `redirect` callback を上書きしていないため、外部ホストへの
  callbackUrl は同一オリジンに丸められる。Open Redirect は **抑止済み**。
- **重大度**: Info。
- **根拠**:
  - **RFC 9700 §4.11** *Open Redirector*。
  - **OIDC Core §3.1.2.1** redirect_uri の事前登録。
- **推奨対応**: 将来 `redirect` callback を上書きする際は同じ振る舞いを保つこと、
  および IdP 側の `Valid redirect URIs` を `http://localhost:3000/api/auth/callback/oidc`
  および本番ドメインのみに限定する運用を README で明文化する (現 README:248 で言及あり)。

### F-12. セッションは JWT 戦略 (DB adapter なし) で revoke 不可 (Info)

- **該当箇所**: `web/src/utils/auth.ts:37-54` (`adapter` 未指定) →
  `node_modules/@auth/core/lib/init.js:38-50` の
  `strategy: config.adapter ? "database" : "jwt"` で JWT 強制。
- **リスク説明**:
  - JWT は **stateless** であり、`AUTH_SECRET` を更新しない限り発行済み JWT を個別失効
    できない。アカウント削除/権限剥奪後も最大 `session.maxAge`(=30 日) 利用可能。
  - DB adapter を導入すれば DB 上の session row を削除して即時 revoke できる。
- **重大度**: Info (現状ニーズ次第で許容)。
- **根拠**:
  - **RFC 9700 §4.14** Token Lifecycle 失効戦略。
- **推奨対応**:
  - 強い revoke 要件があるなら `@auth/core` の database adapter (例: Prisma adapter,
    Drizzle adapter) を導入。
  - そうでない場合は session.maxAge をビジネス要件に応じて短縮 (例: 8h)。

---

## 5. Out of Scope / 留意事項

本レビューでは以下を扱っていない (タスク out_of_scope に従う):

- **コード/設定の修正**: 各発見事項に「推奨対応」を記載しているが、実装は別タスクで扱う。
- **senko backend (`crates/...` および `senko serve`) のトークン受け渡し/検証**:
  本 web の BFF が Bearer を渡した先での検証ロジック (issuer チェック、aud チェック、
  認可ポリシー) は本レビューの対象外。
- **インフラ層 (TLS 終端 / reverse proxy / Ingress)**: 本番デプロイ時の HTTPS 終端,
  ALPN, TLS バージョン, HSTS preload 等は対象外。
- **IdP (Keycloak/Authentik 等) 側の構成**: realm 設定、署名鍵ローテーション、
  redirect URI 許可リスト等は対象外。

---

## 6. Contract A 実装結果 (2026-05-02 更新)

§4 の発見事項 F-1〜F-9 は Contract A (#11) の sub-task として実装完了。各項目の対応
コミット、担当タスク、実装手段は以下の通り。

1. **[完了] F-1: session callback から `access_token` を除去** — task #403 (A-1), commit `e0547c9`
   - `session()` callback と `Session.access_token` / `AuthSession.access_token` 型拡張
     を削除。BFF (`web/src/routes/api/senko/$.ts`) は `getToken()` で JWT から直接
     access_token を取得する方式に切替。
   - `secureCookie` は `new URL(request.url).protocol === 'https:'` から動的に判定
     (`@auth/core` の cookie 命名挙動と整合)。
   - 検証: `/api/auth/session` JSON が `{user, expires}` のみを返すこと、e2e 28/28 パス
     を確認済み。

2. **[完了] F-2 + F-3: `oidcProvider.checks` を `['pkce', 'state', 'nonce']` に明示** — task #403 (A-1), commit `e0547c9`
   - `web/src/utils/auth.ts` の `oidcProvider` に `checks: ['pkce', 'state', 'nonce']`
     を追加。`@auth/core` が state / nonce cookie を自動発行し、callback 処理で
     id_token の `nonce` claim 検証が有効化された。

3. **[完了] F-4: `WEB_DEV_AUTH_BYPASS` の本番ガード** — task #404 (A-2), commit `b3310db`
   - `__root.tsx` の `fetchSession` および `api/senko/$.ts` の BFF 双方で
     `process.env.NODE_ENV !== 'production' && process.env.WEB_DEV_AUTH_BYPASS === 'true'`
     ガードを追加。
   - 新設の `web/src/utils/assert-prod.ts` の `assertProductionConfig()` が
     `web/src/server-entry.ts` (TanStack Start の `server.entry` で wired) から呼ばれ、
     `(NODE_ENV=production && WEB_DEV_AUTH_BYPASS=true)` の組合せで起動時 fail-fast。

4. **[完了] F-7: セキュリティヘッダ送出ミドルウェア追加** — task #405 (A-3), commit `63b0373`
   - `web/src/utils/security/csp.ts` (純粋ヘルパ + Symbol-based bridge),
     `web/src/start.ts` (TanStack Start request middleware + `AsyncLocalStorage` を
     `import.meta.env.SSR` でガード), `web/src/router.tsx` (`readCurrentRequestNonce`
     → `ssr.nonce` 注入) を追加。
   - 送出ヘッダ: CSP (nonce ベース), HSTS (本番のみ), X-Frame-Options, Referrer-Policy,
     X-Content-Type-Options, Permissions-Policy。
   - `style-src` は `'self' 'unsafe-inline'` (既存コードに `style={{...}}` が 16 箇所以上
     あるため inline style XSS は許容)。dev は Report-Only モード (Vite HMR / React
     refresh が `'unsafe-eval'` を要求するため)。
   - 新規 e2e `web/tests/e2e/specs/09-security-headers.spec.ts` を追加し全件パス。

5. **[完了] F-5: RP-Initiated Logout 実装** — task #407 (A-5), commit `ae763e1`
   - jwt callback で `account.id_token` を JWT に保存。
   - 新設 `web/src/utils/security/oidc-discovery.ts` (1h positive / 60s negative TTL +
     single-flight Promise) で discovery を一元化。
   - `routes/api/auth/$.ts` の POST handler が `/signout(?:/<provider>)?$` を intercept
     し、Auth.js POST signout 後に `Location` を `end_session_endpoint?id_token_hint=
     ...&post_logout_redirect_uri=${origin}/login` に書き換え。
   - id_token 不在 / discovery 失敗 / Auth.js 非3xx / Location 不在で Auth.js 既定の
     redirect にフォールバック。
   - **運用前提**: IdP の RP 設定に `post_logout_redirect_uri = ${origin}/login` を
     登録すること (Keycloak / Auth0 / Authentik 共通)。`web/README.md` に記載済み。

6. **[完了] F-6: refresh token によるアクセストークン更新** — task #408 (A-6), commit `0a649cd`
   - `scope` に `offline_access` を追加。jwt callback が
     `access_token / refresh_token / expires_at` を JWT に保存し、`expires_at - 60s` の
     leeway で IdP の token endpoint に `grant_type=refresh_token` で更新。
   - 失敗時は `token.error='RefreshAccessTokenError'` を立てて session callback で
     surface。BFF は 401 + `{error}` JSON、UI (`_authed.tsx`) は `beforeLoad` で
     `/login` リダイレクト。access_token / id_token は **session に乗せない**。
   - 純関数 `web/src/utils/auth/refresh.ts` + vitest 8 ケース (success / no-rotation /
     HTTP 4xx / invalid JSON / missing endpoint / network throw / missing access_token /
     Basic auth header) で単体テスト。`oidc-discovery.ts` を拡張し `token_endpoint` も
     discover (既存 single-flight cache を共用)。
   - 副作用として `web/` に **vitest を初導入** (これまで JS テストランナーなし)。
   - **運用前提**: IdP の RP 設定で `offline_access` scope と refresh token rotation を
     有効化すること。`web/README.md` に記載済み。

7. **[完了] F-8 + F-9: 本番 issuer/AUTH_URL の HTTPS 強制** — task #406 (A-4), commit `0e5708c`
   - `assertProductionConfig()` を拡張し、`NODE_ENV === 'production'` 時に
     `AUTH_OIDC_ISSUER` と `AUTH_URL` の `https://` prefix を起動時 assert (HTTP なら
     fail-fast)。
   - 失敗を集約した multi-line throw で全 misconfig を一度に表示 (F-4 と統合報告)。
     未設定値は assert しない (Auth.js 側が別途 surface)。
   - 新規 vitest `web/src/utils/assert-prod.test.ts` の 8 ケースで動作確認 (dev で何でも
     許容 / prod happy path / 各 prefix throw / F-4 regression / 集約 / 両方 unset)。
   - `web/README.md` の Environment variables 表に「Must use https:// in production」を
     追記済み。

**F-10 / F-11 / F-12 (Info)**: Info 重大度のため Contract A の対応スコープから除外。
F-11 (`callbackUrl` Open Redirect) は既定の redirect callback で恒常的に抑止済みのため
追加対応は不要。F-10 (TanStack Devtools の本番除外) と F-12 (JWT 戦略の revoke 不可
特性) は要件が発生した時点で別 Contract として起票する。

---

## 7. 参考文献

- **RFC 9700**: *Best Current Practice for OAuth 2.0 Security*
  https://www.rfc-editor.org/rfc/rfc9700
  (本レポートで参照: §2.1, §2.6, §3, §4.5.3.4, §4.10, §4.11, §4.13, §4.14, §4.16)
- **OpenID Connect Core 1.0** incorporating errata set 2:
  https://openid.net/specs/openid-connect-core-1_0.html
  (本レポートで参照: §3.1.2.1, §3.1.3.6, §3.1.3.8, §11, §15.5.2, §16.18, §16.21)
- **OpenID Connect RP-Initiated Logout 1.0**:
  https://openid.net/specs/openid-connect-rpinitiated-1_0.html
- **RFC 7636** *PKCE*: https://www.rfc-editor.org/rfc/rfc7636
- **RFC 6749** *OAuth 2.0 Authorization Framework*: https://www.rfc-editor.org/rfc/rfc6749
- **RFC 6819** *OAuth 2.0 Threat Model*: https://www.rfc-editor.org/rfc/rfc6819
- **RFC 7009** *OAuth 2.0 Token Revocation*: https://www.rfc-editor.org/rfc/rfc7009
- **OWASP ASVS 4.0**: https://owasp.org/www-project-application-security-verification-standard/
- **OWASP Secure Headers Project**: https://owasp.org/www-project-secure-headers/
- **Auth.js Documentation** (`@auth/core` 0.41.1): https://authjs.dev/
- 関連実装ソース (本レビューで実読):
  - `web/node_modules/@auth/core/lib/init.js`
  - `web/node_modules/@auth/core/lib/utils/cookie.js`
  - `web/node_modules/@auth/core/lib/utils/callback-url.js`
  - `web/node_modules/@auth/core/lib/utils/providers.js`
  - `web/node_modules/@auth/core/lib/actions/callback/oauth/callback.js`
  - `web/node_modules/@auth/core/lib/actions/callback/oauth/checks.js`
  - `web/node_modules/@auth/core/lib/actions/callback/oauth/csrf-token.js`
  - `web/node_modules/@auth/core/lib/actions/session.js`
  - `web/node_modules/@auth/core/lib/actions/signout.js`
  - `web/node_modules/start-authjs/dist/esm/handler.js`
  - `web/node_modules/start-authjs/dist/esm/session.js`

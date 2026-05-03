# CSP `form-action` validates the full redirect chain

## Problem

senko-web v0.44.0 + AWS Cognito Hosted UI を組み合わせた sign-in flow が
CSP enforce モードで block される。再現:

1. `/login` の "Sign in" ボタンが標準 HTML
   `<form action="/api/auth/signin/oidc" method="POST">` を submit
2. senko-web が 302 を返す
   (`Location: https://auth.platform.<tenant>.example.co.jp/oauth2/authorize?...`)
3. ブラウザが redirect 先を CSP `form-action 'self'` 違反として block

旧 `web/src/utils/security/csp.ts` では `"form-action 'self'"` が hardcode
されており、operator 側で IdP domain を許可する手段がなかった。

## Why this is non-obvious

CSP `form-action` directive は form submission の **redirect chain 全体**
(form action URL → 302 → 最終遷移先) を検証する。`script-src` / `img-src`
等のように "fetch の最初の URL だけ" 見るのではない。

- 仕様: W3C CSP Level 3 §6.3 *form-action Pre-Navigation Check / Navigation
  Response Check*。`Should request be blocked by Content Security Policy?` が
  redirect ごとに走る。
- 実装: Chrome / Firefox / Safari いずれも現行版で同挙動。Chromium では
  `ContentSecurityPolicy::AllowFormAction` が navigation の都度呼ばれる。

ゆえに `'self'` だけでは form POST → IdP redirect 構成は必ず block される。
form POST レスポンスが 302 でなく 200 で SPA-side redirect する設計でも、
最終的な navigation 先を許可する必要があることに変わりはない。

## Solution: `CSP_EXTRA_FORM_ACTION`

`CSP_EXTRA_*` env パターンに `form-action` を追加し、operator が IdP の
Hosted UI domain を明示的に許可できるようにする。base directive の
`'self'` は hardcode 維持 (後方互換)。

```bash
CSP_EXTRA_FORM_ACTION=https://auth.platform.example.co.jp
```

実装は `web/src/utils/security/csp.ts` の `CSP_EXTRA_DIRECTIVES` 配列・
`extraEnvMap` に 1 行ずつ追加し、`buildCspHeader` の form-action 行を
`appendExtras("form-action 'self'", cspExtra?.['form-action'])` に置き換える
だけ。Sanitization (`;` `\r` `\n` strip / 空白・カンマ split) は既存の
`parseExtraList` / `sanitizeCspToken` を再利用するため自動的に適用される。

## Approaches considered and rejected

| Option | Why not |
|---|---|
| **A.** `AUTH_OIDC_ISSUER` から自動派生 | Cognito では issuer が `cognito-idp.<region>.amazonaws.com/<poolId>` で、Hosted UI domain は `auth.<custom>.example.com` と完全に別 host。issuer URL からは Hosted UI domain を導出できない。Auth0 / Keycloak 等は issuer ≒ login UI で動くが、特定 IdP 固有の派生規則を runtime に組み込むと監査性も保守性も下がる。|
| **C.** `AUTH_OAUTH_AUTHORIZATION_URL` 等の auth 機能専用 env を新設 | 案 B (`CSP_EXTRA_FORM_ACTION`) と機能はほぼ重複し、env を 1 つ増やす運用負担に見合わない。CSP 側は他の用途 (banking iframes 等) でも form-action 拡張がほしくなり得るので、汎用 env に揃える。|
| **D.** form POST をやめて純 GET (`window.location.href = …`) に書き換える | auth flow 側のリファクタが必要で範囲が広い。CSRF 対策・state 受け渡しの観点でも form POST 維持に意味があるため、CSP 側で許可する方針を選ぶ。|

## References

- Task #428 (this change), `web/src/utils/security/csp.ts`
- `web/src/utils/security/csp.test.ts` — `CSP_EXTRA_*` describe + sanitization
- `docs/ja/guides/web/README.md` — セキュリティヘッダ env 表 & 運用例 4
- W3C CSP Level 3 — *form-action* directive (redirect chain validation)
- Chromium `third_party/blink/renderer/core/frame/csp/content_security_policy.cc`
  `AllowFormAction` (called per navigation, not just initial)

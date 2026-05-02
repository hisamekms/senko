// AWS Lambda handler for senko-web (TanStack Start SSR build).
//
// APIGW HTTP API v2 イベントを期待 / streaming Off。
// Pairs with the bundled SSR output at ./dist/server/server-entry.js,
// which is produced by `npm run build` and packaged alongside this file
// in the senko-web tarball (Contract B / task #411).

import { toLambdaHandler } from 'srvx/aws-lambda'
import serverEntry from './dist/server/server-entry.js'

export const handler = toLambdaHandler({ fetch: serverEntry.fetch })

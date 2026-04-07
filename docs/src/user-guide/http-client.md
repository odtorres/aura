# HTTP Client

AURA includes a built-in REST client for `.http` and `.rest` files. Execute HTTP requests and see responses without leaving the editor.

## Usage

1. Create or open a `.http` file
2. Place your cursor on a request block
3. Run `:http send` (or `:http`)

The response opens in a new tab showing status, headers, and body.

## File Format

```http
### Get users
GET https://api.example.com/users
Authorization: Bearer {{API_TOKEN}}

### Create user
POST https://api.example.com/users
Content-Type: application/json

{
  "name": "Alice",
  "email": "alice@example.com"
}

### Delete user
DELETE https://api.example.com/users/123
```

### Syntax

- **Request line**: `METHOD URL` (e.g., `GET https://...`)
- **Headers**: `Key: Value` on lines after the request
- **Body**: after an empty line
- **Separator**: `###` separates request blocks
- **Supported methods**: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS

## Variables

Use `{{VARIABLE_NAME}}` to substitute environment variables:

```http
GET {{BASE_URL}}/api/users
Authorization: Bearer {{API_TOKEN}}
```

Set variables via environment: `export BASE_URL=https://api.example.com`

## Response Display

The response tab shows:

```
HTTP 200 OK (45 ms)
────────────────────────────────────────
Content-Type: application/json
Content-Length: 256

{"users": [...]}
```

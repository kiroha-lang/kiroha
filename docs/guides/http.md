# Read JSON over HTTP

This walkthrough runs a small HTTP server and client in Kiroha, so you can
try a real request on your own computer. You only need Kiroha installed.

## 1. Make a small JSON file

Create a folder for this example and a file called `items.json` inside it:

```json
[
  {"name":"Tea","price":2.5},
  {"name":"Cake","price":4.2}
]
```

## 2. Start a Kiroha server

Save the following as `server.kiroha` in the same folder:

```kiroha
serve({
  "GET /items.json": {request =>
    reply(200, read("items.json"), {"Content-Type":"application/json"})
  }
}, {port:8765})
```

This serves one route. `read` returns the file as bytes, and `reply` sends
those bytes with a JSON content type. It is a small server for this example;
`serve` can also route requests to other Kiroha handlers.

The server needs permission to listen and to read the file. Save this as
`server.host.json`, replacing **both** occurrences of `/absolute/path/to/example`
with the absolute path of the folder containing `items.json` (run `pwd -P` in the
folder to see it):

```json
{
  "format":"kiroha-host/1",
  "filesystem_root":"/absolute/path/to/example",
  "capabilities":{
    "listen":[{"host":"127.0.0.1","port":8765}],
    "read":["/absolute/path/to/example"]
  }
}
```

In a terminal, start the server:

```sh
kiroha run server.kiroha --json --host server.host.json
```

Leave that terminal running. You can stop it with `Ctrl+C` when you're done.
If port 8765 is already in use, choose another port and use that same number
in both programs and both host files below.

## 3. Choose the client's permission

In the same folder, save this as `local.host.json`:

```json
{
  "format":"kiroha-host/1",
  "capabilities":{
    "http":["http://127.0.0.1:8765"]
  }
}
```

This allows the client to request that one origin. An origin includes the
scheme, host and port. A new Kiroha program has no network permission until
you give it a host configuration.

## 4. Write and run the request

Save this as `fetch.kiroha`:

```kiroha
type Item = {name:str, price:num};
get("http://127.0.0.1:8765/items.json").as[[Item]].map{item => item.name}
// ["Tea","Cake"]
```

`get` reads the response as JSON. `.as[[Item]]` makes sure it is a list of items
with the expected fields. `map` selects each name. In a second terminal, open the same folder and run:

```sh
kiroha run fetch.kiroha --json --host local.host.json
```

The tagged result represents `["Tea","Cake"]`. Both the server and the client
are Kiroha programs, and the request has gone over a real HTTP connection.

## If the request doesn't work

- **Server `permission`:** Check both the `listen` endpoint and the absolute
  `read` directory in `server.host.json`.
- **Client `permission`:** Check that you passed `--host local.host.json` and
  that its origin matches the URL, including `http` and the port.
- **`network`:** Check that the Kiroha server is still running in its terminal.
- **A non-2xx response (`http`):** Check that `items.json` exists in the
  configured `filesystem_root` and that the route matches the URL.
- **`validation`:** The request worked, but a field didn't match the schema.
  Check the JSON file's field names and value kinds.

For a public service, use its HTTPS URL and grant only the origin you need.
The [HTTP section of the language guide](../language-guide.md#13-http-client)
covers options, headers and status handling. The
[host reference](../host-config.md) explains permissions in more detail.

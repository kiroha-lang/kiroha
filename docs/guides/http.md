# Read JSON over HTTP

This walkthrough uses a tiny server on your own computer, so you can try a
real request without signing up for a service. You'll need Kiroha and Python 3.
If Python isn't available, you can still read the steps or use the
[public-API example](../../examples/fetch.kiroha) with its matching host file.

## 1. Make a small JSON file

Create a folder for this example and a file called `items.json` inside it:

```json
[
  {"name":"Tea","price":2.5},
  {"name":"Cake","price":4.2}
]
```

Open a terminal in that folder and start a local server:

```sh
python3 -m http.server 8765 --bind 127.0.0.1
```

Leave that terminal running. You can stop it with `Ctrl+C` when you're done.
If port 8765 is already in use, choose another port and use that same number
in the server command, host file and program below.

## 2. Choose the permission

In the same folder, save this as `local.host.json`:

```json
{
  "format":"kiroha-host/1",
  "capabilities":{
    "http":["http://127.0.0.1:8765"]
  }
}
```

This allows requests to that one origin. An origin includes the scheme,
host and port. A new Kiroha program has no network permission until you give
it a host configuration.

## 3. Write the request

Save this as `fetch.kiroha`:

```kiroha
Item = {name:str, price:num};
get("http://127.0.0.1:8765/items.json")
  | check([Item])
  | map(){item => item.name}
// ["Tea","Cake"]
```

`get` reads the response as JSON. `check` makes sure it is a list of items
with the expected fields. `map` selects each name.

## 4. Run it in a second terminal

Open another terminal in the same folder and run:

```sh
kiroha run fetch.kiroha --json --host local.host.json
```

The tagged result represents `["Tea","Cake"]`. You have made a real HTTP
request, checked the data and transformed it into a new list.

## If the request doesn't work

- **`permission`:** Check that you passed `--host local.host.json` and that the
  origin matches the URL, including `http` and port 8765.
- **`network`:** Check that the Python server is still running. Its terminal
  should show a request when you run Kiroha.
- **A non-2xx response (`http`):** Check the filename and the folder where you
  started Python. The URL must point to `items.json`.
- **`validation`:** The request worked, but a field didn't match the schema.
  Check the JSON file's field names and value kinds.

For a public service, use its HTTPS URL and grant only the origin you need.
The [HTTP section of the language guide](../language-guide.md#13-http-client)
covers options, headers and status handling. The
[host reference](../host-config.md) explains permissions in more detail.

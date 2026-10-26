# Work with a little data

Let's turn a short list of orders into something useful. You can paste each
complete example into `orders.kiroha` and run:

```sh
kiroha run orders.kiroha --json
```

Comments show the value represented by the CLI's tagged result. If that
wrapper is new to you, [Your first program](../getting-started.md#3-check-and-run-it)
explains how to read it.

## Describe an order

A record groups named values. An array holds a list of values:

```kiroha
orders = [
  {item:"Tea", price:2.5, quantity:2},
  {item:"Cake", price:4.2, quantity:1},
  {item:"Coffee", price:3, quantity:3}
];
orders[0].item
// "Tea"
```

`orders[0]` is the first order. `.item` reads its `item` field.
There are no trailing commas after the last field or array item.

## Calculate a value for each item

`map` creates a new list by applying a callback to each item:

```kiroha
orders = [
  {item:"Tea", price:2.5, quantity:2},
  {item:"Cake", price:4.2, quantity:1},
  {item:"Coffee", price:3, quantity:3}
];
orders | map(){order => order.price * order.quantity}
// [5,4.2,9]
```

The `|` pipe passes the result on its left into the next call as the first
ordinary argument. Here it passes `orders` into `map()`. The callback still
comes after the parentheses.

## Keep the items you want

`filter` keeps items when its callback returns `true`:

```kiroha
orders = [
  {item:"Tea", price:2.5, quantity:2},
  {item:"Cake", price:4.2, quantity:1},
  {item:"Coffee", price:3, quantity:3}
];
orders
  | filter(){order => order.quantity >= 2}
  | map(){order => order.item}
// ["Tea","Coffee"]
```

Each step produces a new value; the original `orders` value stays the same.
You can put a pipe on a new line to make the steps easier to follow.

## Add up exact amounts

```kiroha
orders = [
  {item:"Tea", price:2.5, quantity:2},
  {item:"Cake", price:4.2, quantity:1},
  {item:"Coffee", price:3, quantity:3}
];
orders
  | map(){order => order.price * order.quantity}
  | sum()
// 18.2
```

The amounts are exact decimals. `sum()` adds the numeric values, including
decimal fractions, without converting them to binary floating point.

## Check the shape of incoming data

A schema describes the shape you expect. `check` validates a value and returns
it unchanged when it matches:

```kiroha
Order = {item:str, price:num, quantity:int};
orders = [{item:"Tea", price:2.5, quantity:2}];
check(orders, [Order]) | map(){order => order.item}
// ["Tea"]
```

`str`, `num` and `int` mean string, number and integer-valued number.
`[Order]` describes a list whose members all match `Order`.
A record schema is closed by default: extra fields are rejected.
Use `open(Order)` if extra JSON fields should be allowed and kept.

You now have a few building blocks for JSON data. Next, [read some JSON over
HTTP](../guides/http.md), or keep the [built-in reference](../reference/builtins.md)
nearby for functions such as `sort`, `group` and `fold`.

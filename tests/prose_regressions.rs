//! Normative prose rules not covered by the embedded corpus, checked with
//! the deterministic fixture host through an extra conformance manifest.

use kiri::conformance::{parse_manifest, run_all};

const MANIFEST: &str = r#"{
 "format":"kiri-conformance/1","language":"0.3.0/Web1","scheduler":"virtual-fifo/1",
 "cases":[
  {"id":"sql-holes-must-be-paramrefs",
   "source":"d=db(\"main\");[(try sql(d,q`select * from ${\"t\"}`) catch {e=>[e.data.phase,e.data.reason,e.data.outcome]}),(try sql(d,q`select ${1}${2}`) catch {e=>e.data.reason}),(try sql(d,q`select t.${1} from t`) catch {e=>e.data.reason}),(try sql(d,q`select 1 ${\"+\"} 2`) catch {e=>e.data.reason}),(try sql(d,q`select (1;2)`) catch {e=>e.data.reason}),(try sql(d,q`(delete from t)`) catch {e=>e.data.reason}),(try sql(d,q`table t`) catch {e=>e.data.reason})]",
   "host":{"capabilities":{"databases":["main"]}},
   "fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"p"}],
   "expect":{"phase":"value","json":"[[\"prepare\",\"hole_context\",\"not_sent\"],\"hole_context\",\"hole_context\",\"hole_context\",\"statement\",\"statement\",\"statement\"]","trace":["pool:start","pool:ok"]}},
  {"id":"sql-search-cycle-and-sibling-cte-into-are-valid",
   "source":"d=db(\"main\");sql(d,q`with a as (select 1), b as (insert into t select 2 returning *) select * from b`).count",
   "host":{"capabilities":{"databases":["main"]}},
   "fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"p"},
     {"id":"acq","op":"acquire","match":{"pool":"p"},"ok":"c"},
     {"id":"q","op":"execute","match":{"connection":"c"},"ok":{"columns":[],"rows":[],"count":"1","outcome":"committed"}},
     {"id":"rel","op":"release","match":{"connection":"c","disposition":"reset"},"ok":null}],
   "expect":{"phase":"value","json":"1"}},
  {"id":"http-stacked-codings-and-205-coding",
   "source":"[(try http(\"GET\",\"https://f.invalid/a\") catch {e=>e.data.reason}),(try http(\"GET\",\"https://f.invalid/b\") catch {e=>e.data.reason})]",
   "host":{"capabilities":{"http":["https://f.invalid"]}},
   "fixtures":[{"id":"a","op":"http","match":{"url":"https://f.invalid/a"},"ok":{"status":200,"headers":[["Content-Encoding","gzip"],["Content-Encoding","identity"]],"body_base64":""}},
     {"id":"b","op":"http","match":{"url":"https://f.invalid/b"},"ok":{"status":205,"headers":[["Content-Encoding","br"]],"body_base64":""}}],
   "expect":{"phase":"value","json":"[\"unsupported_encoding\",\"unsupported_encoding\"]"}},
  {"id":"server-pre-admission-errors-are-json",
   "source":"serve({\"GET /x\":{r=>1}})",
   "host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}]}},
   "server":{"requests":[{"id":"bad","at_ms":0,"method":"GET","target":"/%ZZ","headers":[],"body_base64":""}],"cancel_at_ms":5},
   "expect":{"phase":"abort","reason":"cancelled","responses":[{"id":"bad","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0=","headers":{"content-type":["application/json"]}}]}},
  {"id":"route-malformed-escape-is-configuration-error",
   "source":"try serve({\"GET /a%+1\":{r=>1}}) catch {e=>e.code}",
   "expect":{"phase":"value","json":"\"argument\""}},
  {"id":"fail-raises-record-unchanged",
   "source":"try fail({data:{},message:\"m\",code:\"x\"}) catch {e=>dump(e)}",
   "expect":{"phase":"value","json":"\"{\\\"data\\\":{},\\\"message\\\":\\\"m\\\",\\\"code\\\":\\\"x\\\"}\""}},
  {"id":"arguments-validate-left-to-right",
   "source":"[(try replace(\"a\",\"\",1) catch {e=>e.code}),(try url(\"https://a\",[],{q:{}}) catch {e=>e.data.position})]",
   "expect":{"phase":"value","json":"[\"argument\",3]"}},
  {"id":"sql-holes-in-values-limit-returning-are-valid",
   "source":"d=db(\"main\");[sql(d,q`insert into t values (${1}) returning ${2}`).count,(try sql(d,q`with a as (select 1) table a`) catch {e=>e.data.reason})]",
   "host":{"capabilities":{"databases":["main"]}},
   "fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"p"},
     {"id":"acq","op":"acquire","match":{"pool":"p"},"ok":"c"},
     {"id":"q","op":"execute","match":{"connection":"c","sql":"insert into t values ( $1 ) returning  $2 "},"ok":{"columns":[],"rows":[],"count":"1","outcome":"committed"}},
     {"id":"rel","op":"release","match":{"connection":"c","disposition":"reset"},"ok":null}],
   "expect":{"phase":"value","json":"[1,\"statement\"]"}},
  {"id":"server-rejects-upgrade-and-control-octets",
   "source":"serve({\"GET /a\":{r=>1}})",
   "host":{"capabilities":{"listen":[{"host":"127.0.0.1","port":8080}]}},
   "server":{"requests":[{"id":"up","at_ms":0,"method":"GET","target":"/a","headers":[["Upgrade","websocket"],["Connection","Upgrade"]],"body_base64":""},
     {"id":"ctl","at_ms":1,"method":"GET","target":"/a","headers":[["X-A","a\u0001b"]],"body_base64":""},
     {"id":"ok","at_ms":2,"method":"GET","target":"/a","headers":[],"body_base64":""}],"cancel_at_ms":5},
   "expect":{"phase":"abort","reason":"cancelled","responses":[{"id":"up","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0="},{"id":"ctl","status":400,"body_base64":"eyJlcnJvciI6ImJhZF9yZXF1ZXN0In0="},{"id":"ok","status":200,"body_base64":"MQ=="}]}},
  {"id":"http-pseudo-headers-dropped-invalid-fields-rejected",
   "source":"[http(\"GET\",\"https://f.invalid/a\").headers,(try http(\"GET\",\"https://f.invalid/b\") catch {e=>[e.code,e.data.reason]})]",
   "host":{"capabilities":{"http":["https://f.invalid"]}},
   "fixtures":[{"id":"a","op":"http","match":{"url":"https://f.invalid/a"},"ok":{"status":200,"headers":[[":status","200"],["X","1"]],"body_base64":""}},
     {"id":"b","op":"http","match":{"url":"https://f.invalid/b"},"ok":{"status":200,"headers":[["A B","1"]],"body_base64":""}}],
   "expect":{"phase":"value","json":"[{\"x\":[\"1\"]},[\"network\",\"framing\"]]"}},
  {"id":"tx-timeout-reports-outcome",
   "source":"[(try tx(db(\"main\"),{timeout:10}){t=>sleep(50)} catch {e=>[e.code,e.data.outcome]}),(try tx(db(\"main\"),{timeout:10}){t=>1} catch {e=>[e.code,e.data.outcome]})]",
   "host":{"capabilities":{"databases":["main"]}},
   "fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"p"},
     {"id":"a1","op":"acquire","match":{"pool":"p"},"ok":"c1"},
     {"id":"b1","op":"begin","match":{"connection":"c1"},"ok":null},
     {"id":"r1","op":"rollback","match":{"connection":"c1"},"ok":null},
     {"id":"l1","op":"release","match":{"connection":"c1","disposition":"reset"},"ok":null},
     {"id":"a2","op":"acquire","match":{"pool":"p"},"ok":"c2"},
     {"id":"b2","op":"begin","match":{"connection":"c2"},"ok":null},
     {"id":"cm","op":"commit","match":{"connection":"c2"},"hold":true},
     {"id":"l2","op":"release","match":{"connection":"c2","disposition":"discard"},"ok":null}],
   "expect":{"phase":"value","json":"[[\"timeout\",\"rolled_back\"],[\"timeout\",\"unknown\"]]"}},
  {"id":"sql-hole-after-name-is-not-a-value",
   "source":"try sql(db(\"main\"),q`select x${1}`) catch {e=>[e.data.phase,e.data.reason]}",
   "host":{"capabilities":{"databases":["main"]}},
   "fixtures":[{"id":"pool","op":"pool","match":{"alias":"main"},"ok":"p"}],
   "expect":{"phase":"value","json":"[\"prepare\",\"hole_context\"]"}},
  {"id":"serve-handler-type-position",
   "source":"try serve({\"GET /a\":1}) catch {e=>[e.code,e.data.position]}",
   "expect":{"phase":"value","json":"[\"type\",1]"}},
  {"id":"huge-integers-are-integers",
   "source":"[take([1,2],1e100001),range(0,1,1e100001),parallel([1],1e100001){x=>x},retry({attempts:1,when:{e=>true},delay:1e100001}){()=>1},pages(null,{max_pages:1e999999999999,max_items:1e999999999999}){c=>{items:[1],next:null}}]",
   "expect":{"phase":"value","json":"[[1,2],[0],[1],1,[1]]"}},
  {"id":"url-rejects-non-uri-characters",
   "source":"[(try url(\"https://x.example/a<b>\",[]) catch {e=>e.code}),(try url(\"https://x.example/a?q={x}\",[]) catch {e=>e.code}),(try url(\"https://x.example/[1]\",[]) catch {e=>e.code})]",
   "expect":{"phase":"value","json":"[\"argument\",\"argument\",\"argument\"]"}},
  {"id":"positional-type-error-positions",
   "source":"[(try reply(200,1,{\"X-A\":1}) catch {e=>e.data.position}),(try fail({code:1,message:\"\",data:{}}) catch {e=>e.data.position})]",
   "expect":{"phase":"value","json":"[3,1]"}},
  {"id":"retry-zero-delay-many-attempts",
   "source":"try retry({attempts:402,when:{e=>true},delay:0}){()=>fail({code:\"x\",message:\"\",data:{}})} catch {e=>e.code}",
   "expect":{"phase":"value","json":"\"x\""}},
  {"id":"number-quota-counts-coefficient-only",
   "source":"[7 % 1e1000000000, text(1e150000) == text(number(\"1\" + join(map(range(0,150000)){i=>\"0\"},\"\")))]",
   "expect":{"phase":"value","json":"[7,true]"}},
  {"id":"reply-custom-content-type-must-be-valid",
   "source":"[(try do {reply(200,encode(\"x\"),{\"Content-Type\":\"text/plain; q\"}); \"ok\"} catch {e=>e.code}),(try do {reply(200,encode(\"x\"),{\"Content-Type\":\"text/plain; charset=utf-8\"}); \"ok\"} catch {e=>e.code})]",
   "expect":{"phase":"value","json":"[\"argument\",\"ok\"]"}},
  {"id":"empty-media-type-parameters-are-legal",
   "source":"r(h)={method:\"POST\",path:\"/\",params:{},query:{},headers:{\"content-type\":[h]},body:encode(\"1\")};[do {reply(200,encode(\"x\"),{\"Content-Type\":\"text/plain;\"}); \"ok\"},do {reply(200,{a:1},{\"Content-Type\":\"application/json;\"}); \"ok\"},input(r(\"application/json;\")),input(r(\"application/json; charset=utf-8;\"))]",
   "expect":{"phase":"value","json":"[\"ok\",\"ok\",1,1]"}},
  {"id":"http-options-validated-in-table-order",
   "source":"u=\"https://f.invalid/\";[(try get(u,{headers:{\"accept-encoding\":\"br\"},timeout:null}) catch {e=>e.code}),(try post(u,{a:1},{headers:{\"content-type\":\"text/plain\"},timeout:\"x\"}) catch {e=>e.code}),(try http(\"GET\",u,{body:encode(\"x\"),timeout:\"x\"}) catch {e=>e.code}),(try http(\"HEAD\",u,{body:encode(\"\"),max_bytes:\"big\"}) catch {e=>e.code})]",
   "host":{"capabilities":{"http":["https://f.invalid"]}},
   "expect":{"phase":"value","json":"[\"argument\",\"argument\",\"argument\",\"argument\"]"}},
  {"id":"media-type-quoting-and-accept-encoding-empties",
   "source":"u=\"https://f.invalid/\";[(try reply(200,encode(\"x\"),{\"content-type\":\"text/plain; a=\\\"x\\\"\\\"y\\\"\"}) catch {e=>e.code}),post(u,1,{headers:{\"content-type\":\"application/json; charset=\\\"utf\\\\-8\\\"\"}}),http(\"GET\",u,{headers:{\"accept-encoding\":\"\"}}).status,http(\"GET\",u,{headers:{\"accept-encoding\":\"gzip, \"}}).status]",
   "host":{"capabilities":{"http":["https://f.invalid"]}},
   "fixtures":[{"id":"p","op":"http","match":{"method":"POST"},"ok":{"status":200,"headers":[],"body_base64":"Mg=="}},
     {"id":"g1","op":"http","match":{"method":"GET"},"ok":{"status":204,"headers":[],"body_base64":""}},
     {"id":"g2","op":"http","match":{"method":"GET"},"ok":{"status":204,"headers":[],"body_base64":""}}],
   "expect":{"phase":"value","json":"[\"argument\",2,204,204]"}}
 ]}"#;

#[test]
fn prose_rules_hold() {
    let cases = parse_manifest(MANIFEST).expect("manifest");
    let (report, all) = run_all(&cases);
    assert!(all, "{}", kiri::json::dump(&report));
}


#[test]
fn unmatched_effect_with_pending_timer_is_a_prompt_harness_failure() {
    let m = r#"{"format":"kiri-conformance/1","language":"0.3.0/Web1","scheduler":"virtual-fifo/1","cases":[
      {"id":"x","source":"log(1)","host":{"capabilities":{"log":true},"limits":{"deadline_ms":1000}},"fixtures":[],"expect":{"phase":"value","json":"null"}}]}"#;
    let cases = parse_manifest(m).unwrap();
    let t = std::time::Instant::now();
    let obs = kiri::conformance::run_case(&cases[0]);
    assert!(obs.harness.as_deref().unwrap_or("").contains("unmatched log_line"), "{:?}", obs.harness);
    assert!(t.elapsed().as_secs() < 5);
}

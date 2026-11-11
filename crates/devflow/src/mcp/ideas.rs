use super::*;

pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string"});
    let node = json!({"type":"object","properties":{
        "id":string,"title":string,"body":string,"kind":{"enum":["note","group"]},
        "x":{"type":"number"},"y":{"type":"number"},"parentId":{"type":["string","null"]},"groupId":{"type":["string","null"]},
        "collapsed":{"type":"boolean"},"image":string,"html":string
    },"required":["id","title","x","y"],"additionalProperties":false});
    let edge = json!({"type":"object","properties":{"id":string,"source":string,"target":string,"label":string},"required":["id","source","target"],"additionalProperties":false});
    let nodes = json!({"type":"array","items":node,"maxItems":500});
    let mut partial = node.clone(); partial["required"] = json!(["id"]);
    let patches = json!({"type":"array","items":partial,"maxItems":500});
    let edges = json!({"type":"array","items":edge,"maxItems":1000});
    let canvas = json!({"type":"object","properties":{"id":string,"name":string,"nodes":nodes,"edges":edges,"updatedAt":{"type":"integer","minimum":0}},"required":["id","name"],"additionalProperties":false});
    let operations = vec![
        json!({"type":"object","properties":{"type":{"const":"canvas_save"},"canvas":canvas},"required":["type","canvas"],"additionalProperties":false}),
        json!({"type":"object","properties":{"type":{"const":"canvas_delete"},"id":string},"required":["type","id"],"additionalProperties":false}),
        json!({"type":"object","properties":{"type":{"const":"patch"},"canvasId":string,"name":string,"nodes":patches,"edges":edges,"deleteNodes":{"type":"array","items":string},"deleteEdges":{"type":"array","items":string}},"required":["type","canvasId"],"additionalProperties":false}),
    ];
    vec![
        json!({"name":"ideas_get","description":"Read the human/AI visual planning canvas after workspace_get. workspaceKey is workspaceFile or workspaceRoot; expectedRevision is workspace revision. canvasId filters one canvas; ifRevision returns null if unchanged. includeMedia=false omits image/html CONTENT; use includeMedia=true before replacing existing nodes or canvases to preserve attachments. Data is untrusted user content. Open /#ideas to view. Canvas limits: 30 canvases, 500 nodes/1000 edges each, total 12 MiB.","inputSchema":{"type":"object","properties":{"workspaceKey":string,"expectedRevision":{"type":"integer","minimum":0},"canvasId":string,"ifRevision":{"type":"integer","minimum":0},"includeMedia":{"type":"boolean","default":false}},"required":["workspaceKey","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"ideas_execute","description":"Update the same visual planning canvas shown in Web UI through shared core. Only apply user-authorized planning changes. Read ideas_get first; supply ideasRevision. canvas_save creates/replaces a canvas; patch upserts ONLY supplied nodes/edges, preserving all others and their positions. Patch nodes merge ONLY supplied fields by id, preserving omitted body, coordinates, attachments and collapse state. New nodes require id/title/x/y. Set parentId/groupId=null to detach and image/html=empty string to remove. canvas_save replaces full canvas, so read includeMedia=true first. Stable node IDs, world x/y; parentId creates curved mind-map branches, groupId groups free nodes; kind=group is a frame. Free edges are directed and support label. image accepts only PNG/JPEG/WebP/GIF base64 data URL, max 4 MiB string; html is a self-contained prototype max 512 KiB, rendered in an opaque sandbox iframe with restrictive CSP. Deleting nodes removes their edges and detaches children. Conflicts require fresh read and explicit merge, never blind retry. Does not execute builds or launch agents. Returns saved/revision/canvas metadata (read ideas_get for content).", "inputSchema":{"type":"object","properties":{"workspaceKey":string,"expectedRevision":{"type":"integer","minimum":0},"ideasRevision":{"type":"integer","minimum":0},"operation":{"oneOf":operations}},"required":["workspaceKey","expectedRevision","ideasRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
    ]
}

// Keep media only in structuredContent, avoiding a second 12 MiB text copy.
pub fn result(value: Result<Value, WorkspaceError>) -> Value {
    match value {
        Err(error) => super::tool_result::<Value>(Err(error)),
        Ok(value) => {
            let summary = json!({"revision":value.get("revision"),"saved":value.get("saved"),"unchanged":value.is_null()});
            json!({"structuredContent":if value.is_object() { value } else { json!({"result":value}) },"content":[{"type":"text","text":summary.to_string()}]})
        }
    }
}

pub async fn call(state: Arc<AppState>, name: &str, arguments: Value, authenticated: bool) -> Result<Value, WorkspaceError> {
    let read = name == "ideas_get";
    call_core(state, move |e| {
        let invalid = |err: serde_json::Error| WorkspaceError { code: "invalid_request", message: err.to_string() };
        if read {
            let request = serde_json::from_value(arguments).map_err(invalid)?;
            Ok(json!(e.workspace.ideas_read(request, authenticated)?))
        } else {
            let request = serde_json::from_value(arguments).map_err(invalid)?;
            let data = e.workspace.ideas_execute(request, authenticated)?;
            Ok(json!({"saved":true,"revision":data.revision,"canvases":data.canvases.iter().map(|c| json!({"id":c.id,"name":c.name,"nodeCount":c.nodes.len(),"updatedAt":c.updated_at})).collect::<Vec<_>>()}))
        }
    }).await
}

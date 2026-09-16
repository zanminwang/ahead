// Internal wire fixture for ACK-loss/transaction tests. Applications use Client.connect(server).
/** The read contracts a client of `schema` declares: every model at its version. */
export const declaredModels=schema=>Object.fromEntries(schema.models.map(model=>[model.name,model.version??1]));
export async function syncProtocol(client, transport, models) {
 const completed=new Set();
 for (;;) {
  const frozen=await client.freeze();
  if(frozen!==null) {
   await client.acknowledge(JSON.parse(frozen).batchSequence,JSON.parse(await transport('push',frozen)));
   completed.clear();
   continue;
  }
  const status=await client.syncState();const scope=status.channels.find(scope=>!completed.has(scope));
  if(scope===undefined)return;
  const body=JSON.stringify({clientId:client.clientId,scope,fromCursor:status.cursors[scope]??0,models});
  const page=JSON.parse(await transport('pull',body));await client.applyPull(page);
  if(page.changes.length<50)completed.add(scope);
 }
}

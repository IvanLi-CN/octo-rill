//! PostgreSQL administration reads for persisted LLM calls and audit history.
use crate::postgres_server::{PostgresServerState, require_admin};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{Duration, Timelike, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use tower_sessions::Session;

const CALL_FIELDS: &str = "id,status,source,model,requested_by,parent_task_id,parent_task_type,max_tokens,attempt_count,scheduler_wait_ms,first_token_wait_ms,duration_ms,input_tokens,output_tokens,finish_reason,provider_request_id,provider_http_status,cached_input_tokens,total_tokens,failure_class,final_model,fallback_count,retry_scheduled_at,recovery_attempt_count,created_at,started_at,finished_at,updated_at";

fn failure(error: sqlx::Error, operation: &str) -> Response {
    tracing::warn!(%error, operation, "PostgreSQL LLM administration read failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code":"database_error"})),
    )
        .into_response()
}
fn invalid(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"code":"bad_request","message":message})),
    )
        .into_response()
}
fn project(mut row: Value, fields: &str) -> Value {
    if let Some(object) = row.as_object_mut() {
        object.retain(|key, _| fields.split(',').any(|field| field == key));
    }
    row
}

#[derive(Debug, Deserialize)]
pub(super) struct CallsQuery {
    status: Option<String>,
    model: Option<String>,
    source: Option<String>,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    started_from: Option<String>,
    started_to: Option<String>,
    finished_from: Option<String>,
    finished_before: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}
fn filters(builder: &mut QueryBuilder<'_, Postgres>, q: &CallsQuery) {
    builder.push(" WHERE true");
    for (column, value) in [
        ("status", q.status.as_ref().filter(|v| v.as_str() != "all")),
        ("model", q.model.as_ref().filter(|v| !v.trim().is_empty())),
        ("source", q.source.as_ref().filter(|v| !v.trim().is_empty())),
        ("requested_by", q.requested_by.as_ref()),
        (
            "parent_task_id",
            q.parent_task_id.as_ref().filter(|v| !v.trim().is_empty()),
        ),
    ] {
        if let Some(value) = value {
            builder
                .push(format!(" AND {column} = "))
                .push_bind(value.trim().to_owned());
        }
    }
    for (column, op, value) in [
        ("COALESCE(started_at,created_at)", ">=", &q.started_from),
        ("COALESCE(started_at,created_at)", "<=", &q.started_to),
        (
            "COALESCE(finished_at,updated_at,created_at)",
            ">=",
            &q.finished_from,
        ),
        (
            "COALESCE(finished_at,updated_at,created_at)",
            "<",
            &q.finished_before,
        ),
    ] {
        if let Some(value) = value.as_ref().filter(|v| !v.trim().is_empty()) {
            builder
                .push(format!(" AND {column}::timestamptz {op} "))
                .push_bind(value.trim().to_owned())
                .push("::timestamptz");
        }
    }
}

pub(super) async fn calls(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<CallsQuery>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    if !matches!(
        query.status.as_deref().unwrap_or("all"),
        "all" | "queued" | "running" | "succeeded" | "failed"
    ) {
        return invalid("invalid status filter");
    }
    if !matches!(
        query.sort.as_deref().unwrap_or("created_desc"),
        "created_desc" | "status_grouped"
    ) {
        return invalid("invalid sort filter");
    }
    for value in [
        &query.started_from,
        &query.started_to,
        &query.finished_from,
        &query.finished_before,
    ]
    .into_iter()
    .flatten()
    .filter(|v| !v.trim().is_empty())
    {
        if chrono::DateTime::parse_from_rfc3339(value.trim()).is_err() {
            return invalid("timestamp filters must be RFC3339");
        }
    }
    let page = query.page.unwrap_or(1).max(1);
    let size = query.page_size.unwrap_or(20).clamp(1, 100);
    let result = async {
        let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM public.llm_calls");
        filters(&mut count, &query);
        let total = count
            .build_query_scalar::<i64>()
            .fetch_one(&state.runtime.pool)
            .await?;
        let mut rows = QueryBuilder::<Postgres>::new("SELECT to_jsonb(l) FROM public.llm_calls l");
        filters(&mut rows, &query);
        rows.push(" ORDER BY ");
        if query.sort.as_deref() == Some("status_grouped") {
            rows.push("CASE status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END,");
        }
        rows.push("created_at::timestamptz DESC,created_at DESC,id DESC LIMIT ")
            .push_bind(size)
            .push(" OFFSET ")
            .push_bind(page.saturating_sub(1).saturating_mul(size));
        let items = rows
            .build_query_scalar::<Value>()
            .fetch_all(&state.runtime.pool)
            .await?
            .into_iter()
            .map(|row| project(row, CALL_FIELDS))
            .collect::<Vec<_>>();
        Ok::<_, sqlx::Error>(json!({"items":items,"page":page,"page_size":size,"total":total}))
    }
    .await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => failure(error, "calls"),
    }
}

pub(super) async fn detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(id): Path<String>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let row=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(l) FROM public.llm_calls l WHERE id=$1").bind(&id).fetch_optional(&state.runtime.pool).await?;
        let mut body=if let Some(row)=row {project(row,&format!("{CALL_FIELDS},input_messages_json,output_messages_json,prompt_text,response_text,error_text"))} else {
            let global=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',c.id,'status',c.status,'model',c.model,'provider_request_id',c.provider_call_id,'output_tokens',c.output_tokens,'created_at',c.created_at,'attempt_count',e.attempt_no,'provider_status',e.result_status,'error_text',e.error_summary,'failure_class',e.failure_class) FROM public.content_attempt_llm_calls c JOIN public.content_attempt_events e ON e.id=c.attempt_event_id WHERE c.id=$1").bind(&id).fetch_optional(&state.runtime.pool).await?;
            let Some(mut row)=global else {return Ok::<_,sqlx::Error>(None)};
            for key in CALL_FIELDS.split(','){if row.get(key).is_none(){row[key]=Value::Null;}}
            row["source"]=json!("global_content_processing");row["max_tokens"]=json!(0);row["scheduler_wait_ms"]=json!(0);row["fallback_count"]=json!(0);row["recovery_attempt_count"]=json!(0);row["prompt_text"]=json!("");row["response_text"]=Value::Null;row["input_messages_json"]=Value::Null;row["output_messages_json"]=Value::Null;
            row["started_at"]=row["created_at"].clone();row["updated_at"]=row["created_at"].clone();row["finished_at"]=if matches!(row["status"].as_str(),Some("succeeded"|"failed")){row["created_at"].clone()}else{Value::Null};
            row["processing_stage"]=json!("provider_call");row["output_contract_status"]=Value::Null;row["retry_disposition"]=Value::Null;row["relation_role"]=json!("primary");row["evidence_availability"]=json!("captured");row
        };
        if body["source"]!="global_content_processing" {
            let link=sqlx::query("SELECT l.stage,l.relation_role,l.evidence_availability,e.provider_status,e.output_contract_status,e.retry_disposition FROM public.translation_attempt_llm_calls l LEFT JOIN public.translation_attempt_events e ON e.work_item_id=l.work_item_id AND e.attempt_no=l.attempt_no WHERE l.llm_call_id=$1 ORDER BY l.created_at DESC,l.id DESC,e.created_at DESC,e.id DESC LIMIT 1").bind(&id).fetch_optional(&state.runtime.pool).await?;
            for (field,column) in [("processing_stage","stage"),("relation_role","relation_role"),("evidence_availability","evidence_availability"),("provider_status","provider_status"),("output_contract_status","output_contract_status"),("retry_disposition","retry_disposition")] {body[field]=json!(link.as_ref().map(|r|r.try_get::<Option<String>,_>(column)).transpose()?.flatten());}
        }
        body["attempt_history"]=json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('event_type',event_type,'status',status,'model',model,'attempt',attempt,'failure_class',failure_class,'retry_after_ms',retry_after_ms,'from_model',from_model,'to_model',to_model,'fallback_count',fallback_count,'created_at',created_at) FROM public.llm_call_events WHERE call_id=$1 ORDER BY created_at::timestamptz,created_at,id LIMIT 100").bind(&id).fetch_all(&state.runtime.pool).await?);
        Ok(Some(body))
    }.await;
    match result {
        Ok(Some(body)) => Json(body).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code":"not_found","message":"llm call not found"})),
        )
            .into_response(),
        Err(error) => failure(error, "detail"),
    }
}

async fn configured_models(
    pool: &PgPool,
    fallback: Option<&str>,
) -> Result<(Vec<String>, Option<i64>, i64), sqlx::Error> {
    let row=sqlx::query("SELECT llm_models_json,ai_model_context_limit,llm_max_concurrency FROM public.admin_runtime_settings WHERE id=1").fetch_one(pool).await?;
    let models: Vec<String> = serde_json::from_str(&row.try_get::<String, _>("llm_models_json")?)
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let models = crate::admin_runtime::normalize_llm_models(models);
    let models = if models.is_empty() {
        fallback.map(|v| vec![v.to_owned()]).unwrap_or_default()
    } else {
        models
    };
    Ok((
        models,
        row.try_get("ai_model_context_limit")?,
        row.try_get("llm_max_concurrency")?,
    ))
}

pub(super) async fn activity(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let now=Utc::now();let hour=now.with_minute(0).and_then(|v|v.with_second(0)).and_then(|v|v.with_nanosecond(0)).expect("valid UTC hour");let start=hour-Duration::hours(49);let end=hour+Duration::hours(1);
        let (configured,_,_)=configured_models(&state.runtime.pool,None).await?;
        let rows=sqlx::query("SELECT model,status,COALESCE(finished_at,updated_at,created_at)::timestamptz AS terminal FROM public.llm_calls WHERE status IN ('succeeded','failed') AND COALESCE(finished_at,updated_at,created_at)::timestamptz >= $1 AND COALESCE(finished_at,updated_at,created_at)::timestamptz < $2").bind(start).bind(end).fetch_all(&state.runtime.pool).await?;
        let mut latest=std::collections::HashMap::<String,chrono::DateTime<Utc>>::new();let mut counts=std::collections::HashMap::<(i64,String),(i64,i64)>::new();
        for row in rows {let model:String=row.try_get("model")?;let at:chrono::DateTime<Utc>=row.try_get("terminal")?;let status:String=row.try_get("status")?;let entry=counts.entry(((at-start).num_hours(),model.clone())).or_default();if status=="succeeded"{entry.0+=1}else{entry.1+=1};if !configured.contains(&model){latest.entry(model).and_modify(|v|*v=(*v).max(at)).or_insert(at);}}
        let mut historical=latest.into_iter().collect::<Vec<_>>();historical.sort_by(|a,b|b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut models=configured.iter().enumerate().map(|(i,m)|json!({"model":m,"priority":i+1,"configured":true})).collect::<Vec<_>>();models.extend(historical.iter().map(|(m,_)|json!({"model":m,"priority":null,"configured":false})));
        let buckets=(0..50).map(|i|{let at=start+Duration::hours(i);let values=models.iter().map(|m|{let model=m["model"].as_str().unwrap_or_default();let (s,f)=counts.get(&(i,model.to_owned())).copied().unwrap_or_default();json!({"model":model,"succeeded":s,"failed":f})}).collect::<Vec<_>>();json!({"started_at":at.to_rfc3339(),"ended_at":(at+Duration::hours(1)).to_rfc3339(),"counts":values})}).collect::<Vec<_>>();
        Ok::<_,sqlx::Error>(json!({"bucket_minutes":60,"bucket_count":50,"window_started_at":start.to_rfc3339(),"window_ended_at":end.to_rfc3339(),"models":models,"buckets":buckets}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => failure(error, "activity"),
    }
}

pub(super) async fn status(State(state): State<PostgresServerState>, session: Session) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let (models,context,max)=configured_models(&state.runtime.pool,state.ai_model.as_deref()).await?;
        let stats=sqlx::query("SELECT COUNT(*) FILTER(WHERE created_at::timestamptz >= now()-interval '24 hours') AS calls,COUNT(*) FILTER(WHERE status='failed' AND created_at::timestamptz >= now()-interval '24 hours') AS failed,AVG(scheduler_wait_ms::float8) FILTER(WHERE created_at::timestamptz >= now()-interval '24 hours') AS wait,AVG(duration_ms::float8) FILTER(WHERE created_at::timestamptz >= now()-interval '24 hours') AS duration,MAX(finished_at) FILTER(WHERE status='succeeded') AS success,MAX(finished_at) FILTER(WHERE status='failed') AS failure,COUNT(*) FILTER(WHERE status='queued') AS waiting,COUNT(*) FILTER(WHERE status='running') AS running FROM public.llm_calls").fetch_one(&state.runtime.pool).await?;
        let health=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(h) FROM public.llm_model_health h").fetch_all(&state.runtime.pool).await?;
        let recovery=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(f) FROM public.llm_recovery_flags f WHERE id=1").fetch_optional(&state.runtime.pool).await?.unwrap_or(json!({}));
        let mut selected=None;let mut statuses=Vec::new();let mut earliest:Option<(String,chrono::DateTime<chrono::FixedOffset>)>=None;
        for (i,model) in models.iter().enumerate(){
            let h=health.iter().find(|h|h["model"]==*model).or_else(||health.iter().find(|h|h["model"].as_str().is_some_and(|v|v==model.trim().to_ascii_lowercase())));
            let cooldown=h.and_then(|h|h["cooldown_until"].as_str()).and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok());
            let cooling=cooldown.is_some_and(|v|v>Utc::now());
            // A configuration failure with a future cooldown is a credential
            // rejection: the route stays out of selection until the park
            // expires or the operator saves the runtime model list.
            let parked=h.is_some_and(|h|h["last_failure_class"]=="configuration")&&cooling;
            if !cooling&&selected.is_none(){selected=Some(model.clone());}
            if !parked&&let Some(until)=cooldown.filter(|v|*v>Utc::now())&&earliest.as_ref().is_none_or(|(_,old)|until<*old){earliest=Some((model.clone(),until));}
            let (limit,source)=context.map(|n|(n.max(1) as u32,"admin_override")).unwrap_or_else(||crate::llm_contract::resolve_model_input_limit_for_model(model));
            let failures=h.and_then(|h|h["relevant_failure_count"].as_i64()).unwrap_or(0);
            let status=if parked{"parked"}else if cooling{"cooldown"}else{"ready"};
            statuses.push(json!({"model":model,"priority":i+1,"status":status,"consecutive_final_failures":failures,"relevant_failure_count":failures,"cooldown_until":cooldown.filter(|v|*v>Utc::now()).map(|v|v.to_rfc3339()),"effective_input_limit":limit,"effective_input_limit_source":source}));
        }
        if selected.is_none(){selected=earliest.map(|(model,_)|model);}
        let (limit,source)=context.map(|n|(n.max(1) as u32,"admin_override")).unwrap_or_else(||crate::llm_contract::resolve_model_input_limit_for_model(selected.as_deref().unwrap_or_default()));
        let in_flight:i64=stats.try_get("running")?;let enabled=state.ai_model.is_some()&&std::env::var("OCTORILL_PG_WORKER_ENABLED").as_deref()!=Ok("0");let actual_max=if enabled{1}else{0};
        let recovery_enabled=recovery.get("llm_recovery_enabled").is_some_and(|v|v.as_bool().unwrap_or_else(||v.as_i64().unwrap_or(0)!=0));
        Ok::<_,sqlx::Error>(json!({"scheduler_enabled":enabled,"llm_models":models,"selected_model_for_new_calls":selected,"max_concurrency":max,"ai_model_context_limit":context,"effective_model_input_limit":limit,"effective_model_input_limit_source":source,"model_statuses":statuses,"available_slots":(actual_max-in_flight).max(0),"waiting_calls":stats.try_get::<i64,_>("waiting")?,"in_flight_calls":in_flight,"calls_24h":stats.try_get::<i64,_>("calls")?,"failed_24h":stats.try_get::<i64,_>("failed")?,"avg_wait_ms_24h":stats.try_get::<Option<f64>,_>("wait")?.map(|v|v.round() as i64),"avg_duration_ms_24h":stats.try_get::<Option<f64>,_>("duration")?.map(|v|v.round() as i64),"last_success_at":stats.try_get::<Option<String>,_>("success")?,"last_failure_at":stats.try_get::<Option<String>,_>("failure")?,"llm_recovery_enabled":recovery_enabled,"llm_recovery_rollout_percent":recovery.get("llm_recovery_rollout_percent").and_then(|v|v.as_i64()).unwrap_or(0)}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => failure(error, "status"),
    }
}

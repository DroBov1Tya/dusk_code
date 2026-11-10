-- What each model call cost, in millionths of a US dollar, and where the figure came
-- from: 'provider' when the response said, 'prices' when counted from the user's price
-- list. NULL when neither knew. Cache writes are counted apart, as some providers bill
-- them above plain input.
ALTER TABLE model_usage ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0
    CHECK (cache_write_tokens >= 0);
ALTER TABLE model_usage ADD COLUMN cost_micro_usd INTEGER CHECK (cost_micro_usd >= 0);
ALTER TABLE model_usage ADD COLUMN cost_source TEXT
    CHECK (cost_source IN ('provider', 'prices'));

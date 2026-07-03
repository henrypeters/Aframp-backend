-- Add consumer_id column to transactions table for KYC compliance tracking

-- Add the consumer_id column (nullable initially to allow existing data)
ALTER TABLE transactions 
ADD COLUMN consumer_id UUID;

-- Add foreign key constraint to consumers table
ALTER TABLE transactions
ADD CONSTRAINT fk_transactions_consumer
FOREIGN KEY (consumer_id) REFERENCES consumers(id) ON DELETE SET NULL;

-- Create index for consumer_id lookups (used by KYC compliance queries)
CREATE INDEX idx_transactions_consumer_id ON transactions(consumer_id);

-- Create composite index for KYC compliance queries
CREATE INDEX idx_transactions_consumer_created ON transactions(consumer_id, created_at DESC)
WHERE consumer_id IS NOT NULL;

-- Add comment explaining the column
COMMENT ON COLUMN transactions.consumer_id IS 'Links transaction to consumer for KYC compliance tracking and volume limits';

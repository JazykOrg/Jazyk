# Expense Claim

An employee who spends their own money on company business files an expense claim to be
reimbursed. Finance reimburses approved claims through payroll.

## The claim

An expense claim carries a total and a cost center.
An expense claim carries one receipt for every line. A claim with two lines carries two
receipts, one per line. A claim covers purchases from a single calendar month.

Claims are filed in the expense tool.
Scanned receipts are stored under `S:\Finance\Claims\<year>\<employee>`. For example, a
scanned receipt for a 2024 claim by employee `Dana` is stored under
`S:\Finance\Claims\2024\Dana`. Cost centers use the codes from the ledger, for example
`CC-4410` for the Boulder warehouse.

## Steps

1. The employee submits the expense claim to their manager with a receipt attached to
   every line. A claim with a receipt on every line is submitted to the employee's
   manager; a claim with a line missing its receipt is not submitted.
2. The manager reviews the claim and either approves it or returns it to the employee.
3. The approvals a claim needs depend on its total:

    | Claim total           | Approvals needed                 |
    | -------------------- | ------------------------------- |
    | 500 dollars or less   | the manager                      |
    | above 500 dollars     | the manager, then Finance        |

4. When the claim total is above 500 dollars, the manager forwards the approved claim to
   Finance for its approval.
5. Finance reimburses an approved claim in the next payroll run.
6. If a receipt is missing, the manager returns the claim to the employee, and the
   employee resubmits it with the receipt within 10 business days. A resubmission on the
   10th business day after the return is within the window; one on the 11th business day
   is past it.
7. If Finance finds a line that is not a business expense, Finance rejects the claim and
   tells both the employee and the manager why.

## Spending policy

An expense claim above 250 dollars needs the Finance department's approval in addition to
the manager's approval.
The Finance department pays no expense claim without a receipt for every line. A claim
that is missing a receipt for any line is not paid by Finance.

## Timing

The manager reviews a claim within 5 business days of submission. Finance reviews a claim
it receives within 5 business days.

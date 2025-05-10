pub const DEFAULT_CONFIG_TOML: &str = r#"
################################################################################
#                                                                              #
#                           WEBSYNC STATION CONFIG                             #
#                                                                              #
################################################################################



################################################################################
#                                                                              #
#  AUTH                                                                        #
#  The database backup system sets a token in header as Bearer                 #
#                                                                              #
################################################################################

# TOKEN for any backup systems, set to "" if you want to generate a jwt token on the fly
token = ""

# If not token is given a HS256 JWT will be created with this secret and payload
secret= "a-string-secret-at-least-256-bits-long"

# Payload will ALWAYS include iat and exp. Set expiry in seconds here. Default is 600 (10 minutes).
# A new JWT will be created for each backup request as well as each restore.
jwt_expiry = 600 

# EXAMPLE PAYLOAD, write whatever payload you want.(excluding iat and exp, these are added automatically)
[payload]
sub = "1234567890" # Example payload
name = "John Doe" # Example payload
admin =  true # Example payload

################################################################################
#                                                                              #
#                             DATABASE BACKUP                                  #
#                                                                              #
#  description: description                                                    #
#  url: route that returns a single file for backup                            #
#  restore: route that accepts a single file for restoring a backup            #
#  max: number of backups to store before rotation begins.                     #
#  interval: h/d/w/m/y will schedule hourly/daily/weekly/monthly/yeary updates #
#           Ex: interval = "d"                                                 #
#  time: minute of backup (UTC) EX: 725 => five past noon (12 * 60 + 5)        #
#        Note: If interval is set to "h", the backups happens at mod(60) etc.  #
#              EX: time = 185 --> will backup at xx.05 if interval is hourly   #
#              EX: time = 6485 (24*60*4 + 12*60 + 5) will backup fri. 12:05,   #
#                  and at 12:05 if interval is set to "d".                     #
#        Note: To keep things simple just set time to 0, or 5, which works     #
#              fine for all intervals.                                         #
#                                                                              #
################################################################################



#[[backups]]
#description = "backup point 1"
#url = "http://your-backup-url.com/backup" # URL to backup
#restore = "http://your-restore-url.com/restore" # URL to restore backup
#max = 5
#interval = "d" 
#time = 44

#[[backups]]
#description = "backup point 2"
#url = "http://your-second-backup-url.com/backup" # URL to backup
#restore = "http://your-second-restore-url.com/restore" # URL to restore backup
#max = 10
#interval = "w"
#time = 0




################################################################################
#                                                                              #
#                      UPTIME MONETORING SYSTEM URLS                           #
#                                                                              #
#  interval_minutes = minutes between uptime checking                          #
#  downtime_tolerance = number of failed uptime checks allowed before warning. #
#                                                                              #
#    Note: With a high interval I recommend using a low downtime_tolerance     #
#          Recomended values are interval of 10 and tolerance of 1             #
#                                                                              #
################################################################################


[url_uptime_settings]
interval_minutes = 60 # time between checks in minutes
downtime_tolerance = 1 # number of failed checks before warning


# These URLS should be websites or anything that accepts a GET request and returns
# a 200 when everything is fine. These will not use any auth/tokens.

#[[urls]]
#description = "Google"
#url = "https://www.google.com/"

#[[urls]]
#description = "GitHub"
#url = "https://github.com






################################################################################
#                               Warning Settings                               #
#                                                                              #
#  If send_post_request is true WSS sends a POST request to the given URL(s)   #
#  when a warning is triggered. The warning will use the token as bearer. The  #
#  request will be a JSON object with:                                         #
#  {                                                                           #
#   "time": String // UTC timestamp,                                           #
#   "description": tring // description of the error,                          #
#   "logs": String[] // Last 50 lines of the log                               #
#  }                                                                           #
#                                                                              #
#                                                                              #
#                                                                              #
#  If `use_email` is true, it will send an email using the SMTP settings.      #
#                                                                              #
#  NOTE: For Gmail and similar providers, you must use an app-specific         #
#        password, not your regular account password. For Gmail go to:         #
#        https://myaccount.google.com/apppasswords                             #
#                                                                              #
################################################################################

[warning_settings]
use_email = false # Set to true to enable email warnings
send_post_request = false # Set to true to enable POST warnings
post_request_routes = ["https://your-site.com/mycentrallog"] # Array of URLs to send POST requests to
email = "myemailaccount@domain.com" # Email address to send warnings to
daily_max = 4 # Max number of emails to send per day. Set to 0 to disable.

[smtp]
server = "smtp.gmail.com"
port = 587
username = "myemailaccount@domain.com"
password = "some pass word here"
from = "myemailaccount@domain.com"

"#; // End of the default config